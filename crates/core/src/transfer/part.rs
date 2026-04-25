//! Part-level types for multipart uploads.

/// A single part to be uploaded.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Part {
    /// 1-indexed part number (S3 requirement).
    pub number: u32,
    /// Number of bytes in this part.
    pub size: u64,
}

/// Result of a successfully uploaded part.
#[derive(Debug, Clone)]
pub(crate) struct PartResult {
    /// `ETag` returned by S3 for this part.
    pub etag: String,
    /// 1-indexed part number.
    pub number: u32,
}
