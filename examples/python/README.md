# s3z FastAPI Example

A FastAPI server that wraps s3z's Python bindings for S3 operations.

## Setup

```bash
# Start Garage (credentials and region come from root mise.toml)
docker compose --profile garage up -d

# Build the native wheel + install deps
mise run install

# Run (S3_BUCKET, S3_ENDPOINT derived from root mise env)
mise run dev
```

Or via Docker Compose from the repo root:

```bash
docker compose --profile python up
```

## Endpoints

API docs at `http://localhost:8000/docs`.
