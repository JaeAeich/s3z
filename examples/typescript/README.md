# s3z Elysia Example

An Elysia (Bun) server that wraps s3z's Node.js bindings for S3 operations.

## Setup

```bash
# Start Garage (credentials and region come from root mise.toml)
docker compose --profile garage up -d

# Install deps (bun resolves s3z native module via file: link)
bun install

# Build the native module
cd ../../crates/node && napi build --release && cd -

# Run
bun run dev
```

Or via Docker Compose from the repo root:

```bash
docker compose --profile typescript up
```

## Endpoints

API docs at `http://localhost:3000/docs`.
