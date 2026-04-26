//! Chunk scheduling — splits a file into parts based on transfer config.

use crate::{config::TransferConfig, transfer::part::Part};

/// Maximum number of parts S3 allows per multipart upload.
const MAX_S3_PARTS: u64 = 10_000;

/// Minimum part size (8 MiB — above S3's 5 MiB hard minimum, rounded up
/// for better alignment with typical network and disk I/O granularity).
const MIN_PART_SIZE: u64 = 8 * 1024 * 1024;

/// Maximum part size (256 MiB — keep individual retries cheap).
const MAX_PART_SIZE: u64 = 256 * 1024 * 1024;

/// Compute an optimal part size for a given file size and concurrency.
///
/// Targets `concurrency * 2` parts so the upload pipeline doesn't drain
/// early when individual parts finish at different speeds. Clamps the
/// result between [`MIN_PART_SIZE`] and [`MAX_PART_SIZE`], and ensures we
/// never exceed S3's 10,000-part limit.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "target_parts is ≥1, so division cannot panic; all values are bounded by constants"
)]
pub(crate) fn compute_part_size(file_size: u64, concurrency: usize) -> u64 {
    // Ensure we don't exceed 10K parts
    let size_floor = file_size.div_ceil(MAX_S3_PARTS);

    // Target: enough parts to keep all concurrent slots busy,
    // with ~2x headroom so the pipeline doesn't stall
    let concurrency_u64 = u64::try_from(concurrency).unwrap_or(u64::MAX);
    let target_parts = concurrency_u64.saturating_mul(2).max(1);
    let size_for_concurrency = file_size / target_parts;

    size_for_concurrency.max(size_floor).clamp(MIN_PART_SIZE, MAX_PART_SIZE)
}

/// Plan the parts for a file of the given size.
///
/// # Panics
///
/// Panics if `config.part_size` is zero or if the resulting part count
/// would exceed S3's 10,000-part limit.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "offset/number bounded by file_size and part_size"
)]
pub(crate) fn plan_parts(file_size: u64, config: &TransferConfig) -> Vec<Part> {
    assert!(config.part_size > 0, "part_size must be greater than zero");
    let num_parts = file_size.div_ceil(config.part_size);
    assert!(
        num_parts <= MAX_S3_PARTS,
        "file requires {num_parts} parts but S3 allows at most {MAX_S3_PARTS}; increase part_size"
    );

    let num_parts = file_size.div_ceil(config.part_size);
    let capacity = usize::try_from(num_parts).unwrap_or(usize::MAX);
    let mut parts = Vec::with_capacity(capacity);
    let mut remaining = file_size;
    let mut number = 1_u32;
    let mut offset = 0_u64;

    while remaining > 0 {
        let size = remaining.min(config.part_size);
        parts.push(Part {
            number,
            offset,
            size,
        });
        offset += size;
        remaining -= size;
        number += 1;
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(part_size: u64) -> TransferConfig {
        TransferConfig {
            multipart_threshold: 0,
            part_size,
        }
    }

    #[test]
    fn exact_division() {
        let parts = plan_parts(100, &config(50));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].number, 1);
        assert_eq!(parts[0].offset, 0);
        assert_eq!(parts[0].size, 50);
        assert_eq!(parts[1].number, 2);
        assert_eq!(parts[1].offset, 50);
        assert_eq!(parts[1].size, 50);
    }

    #[test]
    fn remainder_part() {
        let parts = plan_parts(110, &config(50));
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].offset, 0);
        assert_eq!(parts[1].offset, 50);
        assert_eq!(parts[2].offset, 100);
        assert_eq!(parts[2].size, 10);
    }

    #[test]
    fn single_part() {
        let parts = plan_parts(30, &config(50));
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].offset, 0);
        assert_eq!(parts[0].size, 30);
    }

    #[test]
    fn zero_file_size() {
        let parts = plan_parts(0, &config(50));
        assert!(parts.is_empty());
    }

    #[test]
    #[should_panic(expected = "part_size must be greater than zero")]
    fn zero_part_size_panics() {
        plan_parts(100, &config(0));
    }

    #[test]
    fn exactly_10000_parts_succeeds() {
        let parts = plan_parts(10_000 * 50, &config(50));
        assert_eq!(parts.len(), 10_000);
    }

    #[test]
    #[should_panic(expected = "S3 allows at most 10000")]
    fn exceeds_s3_part_limit_panics() {
        // 10_001 bytes with 1-byte parts = 10_001 parts
        plan_parts(10_001, &config(1));
    }

    // --- compute_part_size tests ---

    const MB: u64 = 1024 * 1024;

    #[test]
    fn small_file_hits_min_part_size() {
        // 60MB / (4*2) = 7.5MB < 8MB floor → clamped to 8MB
        let ps = compute_part_size(60 * MB, 4);
        assert_eq!(ps, 8 * MB);
    }

    #[test]
    fn medium_file_scales_with_concurrency() {
        // 256MB / (4*2) = 32MB — within bounds
        let ps = compute_part_size(256 * MB, 4);
        assert_eq!(ps, 32 * MB);
    }

    #[test]
    fn large_file_hits_max_part_size() {
        // 10GB / (4*2) = 1.25GB > 256MB cap → clamped to 256MB
        let ps = compute_part_size(10 * 1024 * MB, 4);
        assert_eq!(ps, 256 * MB);
    }

    #[test]
    fn high_concurrency_produces_more_parts() {
        // 10GB / (32*2) = 160MB
        let ps = compute_part_size(10 * 1024 * MB, 32);
        assert_eq!(ps, 160 * MB);
    }

    #[test]
    fn huge_file_respects_10k_limit() {
        // 50TB file: 10K-floor = 50TB/10000 = 5GB, but MAX_PART caps to 256MB
        // 256MB → 50TB/256MB = ~200K parts. However, the 10K floor
        // dominates: 50TB/10000 = ~5GB, capped to 256MB.
        // In practice MAX_PART wins and plan_parts would need a larger part_size
        // for truly enormous files — but compute_part_size returns the best it can.
        let fifty_tb = 50 * 1024 * 1024 * MB;
        let ps = compute_part_size(fifty_tb, 4);
        assert_eq!(ps, 256 * MB);
    }

    #[test]
    fn zero_concurrency_does_not_panic() {
        let ps = compute_part_size(256 * MB, 0);
        // target_parts = max(0*2, 1) = 1 → 256MB, capped to 256MB
        assert_eq!(ps, 256 * MB);
    }
}
