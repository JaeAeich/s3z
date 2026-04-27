<p align="center">
  <picture>
    <source
      media="(prefers-color-scheme: dark)"
      srcset="https://raw.githubusercontent.com/JaeAeich/s3z/main/public/banner-dark.svg">
    <source
      media="(prefers-color-scheme: light)"
      srcset="https://raw.githubusercontent.com/JaeAeich/s3z/main/public/banner-cream.svg">
    <img
      alt="s3z — S3 ops, but fearlessly fast"
      src="https://raw.githubusercontent.com/JaeAeich/s3z/main/public/banner-cream.svg"
      width="800">
  </picture>
</p>

<p align="center">
  <strong>S3 ops, but fearlessly fast.</strong><br>
  <sub>Built in Rust. Streaming. Parallel. No bloat.</sub>
</p>

---

A lightweight, high-throughput S3 client built on raw HTTP + SigV4 signing.
No AWS SDK — just `reqwest`, `aws-sigv4`, and `tokio`.

## Install

```bash
cargo add s3z
```

## Quick start

```rust
use s3z::{Config, S3Client, UploadRequest, auth::CredentialSource};

#[tokio::main]
async fn main() -> s3z::error::Result<()> {
    let client = S3Client::new(
        Config::new("us-east-1", CredentialSource::Env),
    ).await?;

    let result = client
        .upload(UploadRequest::new(
            vec!["./data".into()],
            "my-bucket",
            "uploads/",
        ))
        .await?;

    for f in &result.files {
        println!("{} → s3://{} ({} parts)", f.source.display(), f.key, f.parts);
    }

    Ok(())
}
```

### S3-compatible backends

```rust
use s3z::{Config, S3Client, auth::CredentialSource};

let config = Config::with_endpoint(
    "us-east-1",
    CredentialSource::Static {
        access_key: "minioadmin".into(),
        secret_key: "minioadmin".into(),
    },
    "http://localhost:9000", // MinIO, SeaweedFS, Garage, etc.
);
```

## License

[MIT](https://github.com/JaeAeich/s3z/blob/main/LICENSE)
