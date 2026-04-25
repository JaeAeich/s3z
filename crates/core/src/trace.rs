//! Conditional tracing macros — compile to no-ops when the `tracing` feature is
//! disabled.

#[cfg(feature = "tracing")]
pub(crate) use tracing::{debug, info, warn};

macro_rules! maybe_info {
    ($($arg:tt)*) => {
        #[cfg(feature = "tracing")]
        $crate::trace::info!($($arg)*);
    };
}

macro_rules! maybe_debug {
    ($($arg:tt)*) => {
        #[cfg(feature = "tracing")]
        $crate::trace::debug!($($arg)*);
    };
}

macro_rules! maybe_warn {
    ($($arg:tt)*) => {
        #[cfg(feature = "tracing")]
        $crate::trace::warn!($($arg)*);
    };
}

pub(crate) use maybe_debug;
pub(crate) use maybe_info;
pub(crate) use maybe_warn;
