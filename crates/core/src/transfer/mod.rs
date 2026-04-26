//! Transfer internals — part management, scheduling, multipart orchestration.

pub(crate) mod download;
pub(crate) mod multipart;
pub(crate) mod part;
pub(crate) mod pool;
pub(crate) mod scheduler;
