# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and
this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Core S3 library with batch download, upload, and list operations
- CLI binary (`s3z`) with `download`, `upload`, and `ls` subcommands
- Node.js bindings via NAPI-RS
- Python bindings via PyO3
- Adaptive per-file concurrency based on object size
- Multipart upload and download with configurable thresholds
- SigV4 request signing (no AWS SDK dependency)
- Support for S3-compatible backends (MinIO, Garage, SeaweedFS, R2)
