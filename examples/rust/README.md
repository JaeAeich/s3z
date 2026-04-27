# s3z Axum Example

An Axum server using s3z directly as a Rust crate — no FFI overhead.

## Setup

```bash
# Start Garage (credentials and region come from root mise.toml)
docker compose --profile garage up -d

# Build and run
cd examples/rust
cargo run --release
```

Or via Docker Compose from the repo root:

```bash
docker compose --profile rust up
```

## Endpoints

API docs at `http://localhost:8080/docs`.
