//! Response models for the API.

use serde::Serialize;
use utoipa::ToSchema;

/// Outcome of a single file upload.
#[derive(Debug, Serialize, ToSchema)]
pub struct UploadFileResult {
    /// S3 object key the file was written to.
    pub key: String,
    /// File size in bytes.
    pub size: u64,
    /// Number of multipart parts used (1 = single PUT).
    pub parts: u32,
    /// `ETag` returned by S3.
    pub etag: String,
}

/// Metadata for a single S3 object.
#[derive(Debug, Serialize, ToSchema)]
pub struct ObjectInfo {
    /// Full object key.
    pub key: String,
    /// Object size in bytes.
    pub size: u64,
    /// `ETag` of the object.
    pub etag: String,
    /// Last-modified timestamp (ISO 8601).
    pub last_modified: String,
}
