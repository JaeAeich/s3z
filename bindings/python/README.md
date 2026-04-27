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

Python bindings for [s3z](https://github.com/JaeAeich/s3z) — a lightweight,
high-throughput S3 library built in Rust.

## Install

```bash
pip install s3z
```

## Quick start

```python
from s3z import Config, S3Client, CredentialSource

config = Config("us-east-1", CredentialSource.Env)
client = S3Client(config)

# Upload
results = client.upload(["./data"], "my-bucket", "uploads/")
for r in results:
    print(f"{r.source} -> s3://{r.key} ({r.parts} parts)")

# Download
files = client.download("my-bucket", "uploads/", "./out")

# List
objects = client.list("my-bucket", "uploads/")
```

### S3-compatible backends

```python
from s3z import Config, S3Client, static_credentials

config = Config(
    "us-east-1",
    static_credentials("access-key", "secret-key"),
    endpoint="http://localhost:9000",  # MinIO, R2, GCS, etc.
)
client = S3Client(config)
```

## License

[MIT](https://github.com/JaeAeich/s3z/blob/main/LICENSE)
