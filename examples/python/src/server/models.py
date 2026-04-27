"""Pydantic response models for the s3z API."""

from __future__ import annotations

from pydantic import BaseModel, Field


class UploadFileResult(BaseModel):
    """Outcome of a single file upload to S3."""

    key: str = Field(description="S3 object key the file was written to.")
    size: int = Field(description="File size in bytes.")
    parts: int = Field(description="Number of multipart parts used (1 = single PUT).")
    etag: str = Field(description="ETag returned by S3.")


class ObjectInfo(BaseModel):
    """Metadata for a single S3 object."""

    key: str = Field(description="Full object key.")
    size: int = Field(description="Object size in bytes.")
    etag: str = Field(description="ETag of the object.")
    last_modified: str = Field(description="Last-modified timestamp (ISO 8601).")


class ErrorDetail(BaseModel):
    """Standard error response body."""

    detail: str = Field(description="Human-readable error message.")
