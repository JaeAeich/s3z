//! Chunk scheduling — splits a file into parts based on transfer config.

use crate::{config::TransferConfig, transfer::part::Part};

/// Maximum number of parts S3 allows per multipart upload.
const MAX_S3_PARTS: u64 = 10_000;

/// Minimum part size (8 MiB — above S3's 5 MiB hard minimum, rounded up
/// for better alignment with typical network and disk I/O granularity).
const MIN_PART_SIZE: u64 = 8 * 1024 * 1024;

/// Maximum part size (256 MiB — keep individual retries cheap).
const MAX_PART_SIZE: u64 = 256 * 1024 * 1024;

/// Compute an optimal part size for uploads.
///
/// Targets `concurrency * 2` parts so the upload pipeline has headroom:
/// when one slot finishes its PUT early, a queued part is ready
/// immediately, avoiding idle time between the variable-latency
/// read → sign → PUT → ack cycles.
///
/// Clamps the result between [`MIN_PART_SIZE`] and [`MAX_PART_SIZE`],
/// and ensures we never exceed S3's 10,000-part limit.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "target_parts is ≥1, so division cannot panic; all values are bounded by constants"
)]
pub(crate) fn compute_part_size(file_size: u64, concurrency: usize) -> u64 {
    // Ensure we don't exceed 10K parts
    let size_floor = file_size.div_ceil(MAX_S3_PARTS);

    // 2× headroom keeps the pipeline fed when individual PUTs finish
    // at different speeds.
    let concurrency_u64 = u64::try_from(concurrency).unwrap_or(u64::MAX);
    let target_parts = concurrency_u64.saturating_mul(2).max(1);
    let size_for_concurrency = file_size / target_parts;

    size_for_concurrency.max(size_floor).clamp(MIN_PART_SIZE, MAX_PART_SIZE)
}

/// Compute an optimal part size for downloads.
///
/// Targets exactly `concurrency` parts — one per concurrent slot.
/// Downloads benefit less from pipeline headroom than uploads because
/// the server pushes data immediately. Fewer parts means fewer HTTP
/// Range requests, each of which carries signing and round-trip overhead.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "target_parts is ≥1, so division cannot panic; all values are bounded by constants"
)]
pub(crate) fn compute_download_part_size(file_size: u64, concurrency: usize) -> u64 {
    let size_floor = file_size.div_ceil(MAX_S3_PARTS);

    let concurrency_u64 = u64::try_from(concurrency).unwrap_or(u64::MAX);
    let target_parts = concurrency_u64.max(1);
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
            multipart_download_threshold: 0,
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

    // --- compute_part_size (upload) tests ---

    const MB: u64 = 1024 * 1024;

    #[test]
    fn upload_small_file_hits_min_part_size() {
        // 60MB / (4*2) = 7.5MB < 8MB floor → clamped to 8MB
        let ps = compute_part_size(60 * MB, 4);
        assert_eq!(ps, 8 * MB);
    }

    #[test]
    fn upload_medium_file_scales_with_concurrency() {
        // 256MB / (4*2) = 32MB — within bounds
        let ps = compute_part_size(256 * MB, 4);
        assert_eq!(ps, 32 * MB);
    }

    #[test]
    fn upload_large_file_hits_max_part_size() {
        // 10GB / (4*2) = 1.25GB > 256MB cap → clamped to 256MB
        let ps = compute_part_size(10 * 1024 * MB, 4);
        assert_eq!(ps, 256 * MB);
    }

    #[test]
    fn upload_high_concurrency_produces_more_parts() {
        // 10GB / (32*2) = 160MB
        let ps = compute_part_size(10 * 1024 * MB, 32);
        assert_eq!(ps, 160 * MB);
    }

    #[test]
    fn upload_huge_file_respects_10k_limit() {
        let fifty_tb = 50 * 1024 * 1024 * MB;
        let ps = compute_part_size(fifty_tb, 4);
        assert_eq!(ps, 256 * MB);
    }

    #[test]
    fn upload_zero_concurrency_does_not_panic() {
        let ps = compute_part_size(256 * MB, 0);
        // target_parts = max(0*2, 1) = 1 → 256MB, capped to 256MB
        assert_eq!(ps, 256 * MB);
    }

    // --- compute_download_part_size tests ---

    #[test]
    fn download_small_file_hits_min_part_size() {
        // 24MB / 4 = 6MB < 8MB floor → clamped to 8MB
        let ps = compute_download_part_size(24 * MB, 4);
        assert_eq!(ps, 8 * MB);
    }

    #[test]
    fn download_medium_file_scales_with_concurrency() {
        // 256MB / 4 = 64MB — within bounds
        let ps = compute_download_part_size(256 * MB, 4);
        assert_eq!(ps, 64 * MB);
    }

    #[test]
    fn download_large_file_fewer_parts_than_upload() {
        // Same file + concurrency: upload gets 32MB parts, download gets 64MB
        let ul = compute_part_size(256 * MB, 4);
        let dl = compute_download_part_size(256 * MB, 4);
        assert!(dl > ul, "download parts should be larger (fewer requests)");
    }

    #[test]
    fn download_zero_concurrency_does_not_panic() {
        let ps = compute_download_part_size(256 * MB, 0);
        assert_eq!(ps, 256 * MB);
    }
}
