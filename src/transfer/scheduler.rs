//! Chunk scheduling — splits a file into parts based on transfer config.

use crate::{config::TransferConfig, transfer::part::Part};

/// Plan the parts for a file of the given size.
///
/// # Panics
///
/// Panics if `config.part_size` is zero.
#[expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) is intentional — parent module is private"
)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "offset/number bounded by file_size and part_size"
)]
pub(crate) fn plan_parts(file_size: u64, config: &TransferConfig) -> Vec<Part> {
    assert!(config.part_size > 0, "part_size must be greater than zero");

    let num_parts = file_size.div_ceil(config.part_size);
    let capacity = usize::try_from(num_parts).unwrap_or(usize::MAX);
    let mut parts = Vec::with_capacity(capacity);
    let mut remaining = file_size;
    let mut number = 1_u32;

    while remaining > 0 {
        let size = remaining.min(config.part_size);
        parts.push(Part {
            number,
            size,
        });
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
        assert_eq!(parts[0].size, 50);
        assert_eq!(parts[1].number, 2);
        assert_eq!(parts[1].size, 50);
    }

    #[test]
    fn remainder_part() {
        let parts = plan_parts(110, &config(50));
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2].size, 10);
    }

    #[test]
    fn single_part() {
        let parts = plan_parts(30, &config(50));
        assert_eq!(parts.len(), 1);
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
}
