###################################################
# Stage 1: Build s3z native module                #
###################################################
FROM rust:1.93-bookworm AS native-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential python3-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock README.md ./
COPY crates crates
COPY bindings bindings

RUN --mount=type=cache,target=/src/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --manifest-path bindings/Cargo.toml -p s3z-node && \
    cp target/release/libs3z_js.so bindings/node/s3z.node

###################################################
# Stage 2: Install deps and build app             #
###################################################
FROM oven/bun:1 AS builder

WORKDIR /app

COPY examples/typescript/package.json examples/typescript/bun.lock* ./
COPY bindings/node/package.json /bindings/node/package.json
COPY --from=native-builder /src/bindings/node/*.node /bindings/node/
COPY --from=native-builder /src/bindings/node/index.js /bindings/node/
COPY --from=native-builder /src/bindings/node/index.d.ts /bindings/node/

# Rewrite the file: reference to point at the copied binding
RUN sed -i 's|file:../../bindings/node|file:/bindings/node|' package.json

RUN bun install --frozen-lockfile || bun install

COPY examples/typescript/src src
COPY examples/typescript/tsconfig.json .

###################################################
# Stage 3: Runtime                                #
###################################################
FROM oven/bun:1-slim

RUN apt-get update && apt-get install -y --no-install-recommends tini ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd --system app && useradd --system --gid app --home /app --shell /bin/sh app \
    && mkdir -p /app \
    && chown -R app:app /app

COPY --from=builder --chown=app:app /app /app
COPY --from=builder --chown=app:app /bindings /bindings

WORKDIR /app
USER app

EXPOSE 3000
ENTRYPOINT ["tini", "--"]
CMD ["bun", "run", "src/index.ts"]
