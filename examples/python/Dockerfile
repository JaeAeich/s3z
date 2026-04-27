# Global ARGs
ARG PY_VERSION=3.14
ARG PY_IMAGE=slim

###################################################
# Stage 1: Build s3z native wheel                 #
###################################################
FROM python:${PY_VERSION}-${PY_IMAGE} AS wheel-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential curl pkg-config && \
    rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME="/usr/local/rustup" \
    CARGO_HOME="/usr/local/cargo" \
    PATH="/usr/local/cargo/bin:$PATH"
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain 1.93.1 --profile minimal

RUN pip install --no-cache-dir maturin

WORKDIR /src
COPY Cargo.toml Cargo.lock README.md ./
COPY crates crates

RUN --mount=type=cache,target=/src/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    maturin build --release \
        --manifest-path crates/python/Cargo.toml \
        --out /wheels

###################################################
# Stage 2: Install dependencies and application   #
###################################################
FROM python:${PY_VERSION}-${PY_IMAGE} AS builder

ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy
ENV UV_NO_DEV=1

WORKDIR /app

COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/

# Sync dependencies only (cached layer)
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=bind,source=examples/python/uv.lock,target=uv.lock \
    --mount=type=bind,source=examples/python/pyproject.toml,target=pyproject.toml \
    uv sync --frozen --no-install-project --no-editable

# Copy and install the application
COPY examples/python/pyproject.toml examples/python/uv.lock ./
COPY examples/python/src src
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --frozen --no-editable

# Install the s3z native wheel (after sync, as it's not in the lockfile)
COPY --from=wheel-builder /wheels /tmp/wheels
RUN --mount=type=cache,target=/root/.cache/uv \
    uv pip install --python .venv /tmp/wheels/*.whl && \
    rm -rf /tmp/wheels

###################################################
# Stage 3: Runtime image                          #
###################################################
FROM python:${PY_VERSION}-${PY_IMAGE}

RUN apt-get update && apt-get install -y --no-install-recommends tini curl && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd --system app && useradd --system --gid app --home /app --shell /bin/sh app \
    && mkdir -p /app \
    && chown -R app:app /app

COPY --from=builder --chown=app:app /app/.venv /app/.venv

ENV PATH="/app/.venv/bin:$PATH"

WORKDIR /app
USER app

EXPOSE 8000
ENTRYPOINT ["tini", "--"]
CMD ["uvicorn", "server.app:app", "--host", "0.0.0.0", "--port", "8000"]
