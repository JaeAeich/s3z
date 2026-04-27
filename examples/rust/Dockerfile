###################################################
# Stage 1: Build the binary                       #
###################################################
FROM rust:1.93-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Copy workspace manifests + lockfile for dependency caching
COPY Cargo.toml Cargo.lock README.md ./
COPY crates crates
COPY examples/rust examples/rust

RUN --mount=type=cache,target=/src/examples/rust/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --manifest-path examples/rust/Cargo.toml && \
    cp examples/rust/target/release/s3z-axum /usr/local/bin/s3z-axum

###################################################
# Stage 2: Minimal runtime                        #
###################################################
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl tini && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd --system app && useradd --system --gid app --home /app --shell /bin/sh app

COPY --from=builder /usr/local/bin/s3z-axum /usr/local/bin/s3z-axum

USER app

EXPOSE 8080
ENTRYPOINT ["tini", "--"]
CMD ["s3z-axum"]
