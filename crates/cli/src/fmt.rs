//! Human-readable formatting utilities.

/// Format a byte count as a human-readable string (B, KiB, MiB, GiB).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::float_arithmetic,
    reason = "display-only float conversion — precision loss is irrelevant for human output"
)]
pub(crate) fn bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;

    if n >= GIB {
        format!("{:.2} GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.2} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.2} KiB", n as f64 / KIB as f64)
    } else {
        format!("{n} B")
    }
}

/// Format throughput as `<size>/s`.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::float_arithmetic,
    reason = "display-only float conversion"
)]
pub(crate) fn throughput(total_bytes: u64, elapsed_secs: f64) -> String {
    if elapsed_secs <= 0.0_f64 {
        return "\u{221e}".into();
    }
    let bps = (total_bytes as f64 / elapsed_secs) as u64;
    format!("{}/s", bytes(bps))
}
