//! Python bindings for s3z.
//!
//! Exposes the core s3z library to Python via `PyO3`. All async operations
//! are run on a shared tokio runtime, blocking the Python thread until
//! completion. For true async Python usage, call these from a thread pool.

use std::path::PathBuf;

use pyo3::{exceptions::PyRuntimeError, prelude::*, types::PyList};

/// Map s3z errors to Python `RuntimeError`.
fn to_py_err(e: &s3z::error::Error) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

/// Shared tokio runtime for blocking on async ops.
fn runtime() -> &'static tokio::runtime::Runtime {
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
    })
}

/// How to obtain AWS credentials.
#[pyclass(eq, from_py_object, name = "CredentialSource")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum PyCredentialSource {
    /// Read from `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` env vars.
    Env,
}

impl PyCredentialSource {
    const fn to_core(&self) -> s3z::auth::CredentialSource {
        match self {
            Self::Env => s3z::auth::CredentialSource::Env,
        }
    }
}

/// Create a static credential source from explicit keys.
#[pyfunction]
const fn static_credentials(access_key: String, secret_key: String) -> PyCredentialSource_ {
    PyCredentialSource_ {
        access_key,
        secret_key,
    }
}

/// Wrapper for static credentials (cannot be an enum variant with data in `PyO3`).
#[pyclass(name = "StaticCredentials", from_py_object)]
#[derive(Debug, Clone)]
struct PyCredentialSource_ {
    access_key: String,
    secret_key: String,
}

impl PyCredentialSource_ {
    fn to_core(&self) -> s3z::auth::CredentialSource {
        s3z::auth::CredentialSource::Static {
            access_key: self.access_key.clone(),
            secret_key: self.secret_key.clone(),
        }
    }
}

/// S3 client configuration.
#[pyclass(name = "Config", from_py_object)]
#[derive(Debug, Clone)]
struct PyConfig {
    inner: s3z::Config,
}

#[pymethods]
impl PyConfig {
    /// Create a new config for the given region using env credentials.
    #[new]
    #[pyo3(signature = (region, credential_source=None, endpoint=None))]
    fn new(
        region: String, credential_source: Option<&Bound<'_, PyAny>>, endpoint: Option<String>,
    ) -> PyResult<Self> {
        let cred_source = if let Some(cs) = credential_source {
            if let Ok(env_cred) = cs.extract::<PyCredentialSource>() {
                env_cred.to_core()
            } else if let Ok(static_cred) = cs.extract::<PyRef<'_, PyCredentialSource_>>() {
                static_cred.to_core()
            } else {
                return Err(PyRuntimeError::new_err(
                    "credential_source must be CredentialSource.Env or StaticCredentials",
                ));
            }
        } else {
            s3z::auth::CredentialSource::Env
        };

        let inner = if let Some(ep) = endpoint {
            s3z::Config::with_endpoint(region, cred_source, ep)
        } else {
            s3z::Config::new(region, cred_source)
        };

        Ok(Self {
            inner,
        })
    }

    /// Get the resolved endpoint URL.
    #[getter]
    fn endpoint_url(&self) -> &str {
        self.inner.endpoint_url()
    }

    /// Get the region.
    #[getter]
    fn region(&self) -> &str {
        &self.inner.region
    }
}

/// The s3z S3 client.
#[pyclass(name = "S3Client", skip_from_py_object)]
#[derive(Debug, Clone)]
struct PyS3Client {
    inner: s3z::S3Client,
}

#[pymethods]
impl PyS3Client {
    /// Create a new client with the given config.
    #[new]
    fn new(config: &PyConfig) -> PyResult<Self> {
        let client = runtime()
            .block_on(s3z::S3Client::new(config.inner.clone()))
            .map_err(|ref e| to_py_err(e))?;
        Ok(Self {
            inner: client,
        })
    }

    /// Upload files/directories to S3.
    #[pyo3(signature = (sources, bucket, prefix, workers=None, concurrency_per_file=None))]
    fn upload(
        &self, sources: &Bound<'_, PyList>, bucket: &str, prefix: &str, workers: Option<usize>,
        concurrency_per_file: Option<usize>,
    ) -> PyResult<Vec<PyFileUploadResult>> {
        let paths: Vec<PathBuf> = sources
            .iter()
            .map(|item| item.extract::<String>().map(PathBuf::from))
            .collect::<PyResult<_>>()?;

        let mut req = s3z::UploadRequest::new(paths, bucket, prefix);
        if let Some(w) = workers {
            req.workers = w;
        }
        if let Some(c) = concurrency_per_file {
            req.concurrency_per_file = c;
        }

        let result = runtime().block_on(self.inner.upload(req)).map_err(|ref e| to_py_err(e))?;

        Ok(result
            .files
            .into_iter()
            .map(|f| {
                PyFileUploadResult {
                    etag: f.etag,
                    key: f.key,
                    parts: f.parts,
                    size: f.size,
                    source: f.source.to_string_lossy().into_owned(),
                }
            })
            .collect())
    }

    /// Download objects under a prefix to a local directory.
    #[pyo3(signature = (bucket, prefix, dest_dir, workers=None, concurrency_per_file=None))]
    fn download(
        &self, bucket: &str, prefix: &str, dest_dir: &str, workers: Option<usize>,
        concurrency_per_file: Option<usize>,
    ) -> PyResult<Vec<PyFileDownloadResult>> {
        let mut req = s3z::DownloadRequest::new(bucket, prefix, dest_dir);
        if let Some(w) = workers {
            req.workers = w;
        }
        if let Some(c) = concurrency_per_file {
            req.concurrency_per_file = c;
        }

        let result = runtime().block_on(self.inner.download(req)).map_err(|ref e| to_py_err(e))?;

        Ok(result
            .files
            .into_iter()
            .map(|f| {
                PyFileDownloadResult {
                    dest: f.dest.to_string_lossy().into_owned(),
                    key: f.key,
                    parts: f.parts,
                    size: f.size,
                }
            })
            .collect())
    }

    /// List objects under a prefix.
    #[pyo3(signature = (bucket, prefix, delimiter=None))]
    fn list(
        &self, bucket: &str, prefix: &str, delimiter: Option<String>,
    ) -> PyResult<Vec<PyObjectInfo>> {
        let mut req = s3z::ListRequest::new(bucket, prefix);
        if let Some(d) = delimiter {
            req = req.with_delimiter(d);
        }

        let mut paginator = self.inner.list(req);
        let objects = runtime().block_on(paginator.collect_all()).map_err(|ref e| to_py_err(e))?;

        Ok(objects
            .into_iter()
            .map(|o| {
                PyObjectInfo {
                    key: o.key,
                    size: o.size,
                    etag: o.etag,
                    last_modified: o.last_modified,
                }
            })
            .collect())
    }
}

/// Result of a single file upload.
#[pyclass(name = "FileUploadResult", get_all, skip_from_py_object)]
#[derive(Debug, Clone)]
struct PyFileUploadResult {
    /// `ETag` returned by S3.
    etag: String,
    /// S3 key.
    key: String,
    /// Number of parts used.
    parts: u32,
    /// File size in bytes.
    size: u64,
    /// Local source path.
    source: String,
}

#[pymethods]
impl PyFileUploadResult {
    fn __repr__(&self) -> String {
        format!(
            "FileUploadResult(key='{}', size={}, parts={}, source='{}')",
            self.key, self.size, self.parts, self.source
        )
    }
}

/// Result of a single file download.
#[pyclass(name = "FileDownloadResult", get_all, skip_from_py_object)]
#[derive(Debug, Clone)]
struct PyFileDownloadResult {
    /// Local path the file was written to.
    dest: String,
    /// S3 key that was downloaded.
    key: String,
    /// Number of parts used.
    parts: u32,
    /// File size in bytes.
    size: u64,
}

#[pymethods]
impl PyFileDownloadResult {
    fn __repr__(&self) -> String {
        format!(
            "FileDownloadResult(key='{}', size={}, parts={}, dest='{}')",
            self.key, self.size, self.parts, self.dest
        )
    }
}

/// Metadata for a single S3 object.
#[pyclass(name = "ObjectInfo", get_all, skip_from_py_object)]
#[derive(Debug, Clone)]
struct PyObjectInfo {
    /// Object key.
    key: String,
    /// Object size in bytes.
    size: u64,
    /// `ETag`.
    etag: String,
    /// Last modified timestamp.
    last_modified: String,
}

#[pymethods]
impl PyObjectInfo {
    fn __repr__(&self) -> String {
        format!("ObjectInfo(key='{}', size={}, etag='{}')", self.key, self.size, self.etag)
    }
}

/// s3z — S3 ops, but fearlessly fast.
#[pymodule(name = "s3z")]
mod s3z_py {
    #[pymodule_export]
    use super::{
        PyConfig,
        PyCredentialSource,
        PyCredentialSource_,
        PyFileDownloadResult,
        PyFileUploadResult,
        PyObjectInfo,
        PyS3Client,
        static_credentials,
    };
}
