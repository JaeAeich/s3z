# Contributing to s3z

Thanks for your interest in contributing! This guide covers the workflow and
tooling you need to get started.

## Prerequisites

- [Rust](https://rustup.rs/) (edition 2024, see `mise.toml` for exact version)
- [mise](https://mise.jdx.dev/) — manages tool versions and task runner
- [Docker](https://docs.docker.com/get-docker/) — for local S3 backends

Install all dev tools in one shot:

```sh
mise install
```

## Local development

### Start S3 backends

```sh
docker compose up -d
```

This spins up MinIO (`:9000`), SeaweedFS (`:9500`), and Garage (`:9700`) with
auto-created buckets.

### Build and test

```sh
cargo build --workspace
cargo test --workspace
```

### Pre-commit hooks

The repo uses [Lefthook](https://github.com/evilmartians/lefthook) for
pre-commit checks. Install hooks once:

```sh
lefthook install
```

Hooks run automatically on commit and include:

| Check | What it does |
| ----- | ------------ |
| `editorconfig` | Fixes whitespace / encoding |
| `toml-hygiene` | Sorts and formats TOML files |
| `markdownlint` | Lints Markdown files |
| `rustfmt` | Formats Rust code (nightly) |
| `clippy` | Lints Rust code (deny warnings) |
| `commitlint` | Enforces conventional commit messages |

### Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/). The format
is enforced by commitlint on every commit.

```text
<type>(<scope>): <description>

[optional body]
```

Common types: `feat`, `fix`, `perf`, `refactor`, `docs`, `chore`, `test`,
`ci`, `build`.

Scopes typically match crate names: `core`, `cli`, `node`, `python`, `bench`,
`transfer`.

### Running benchmarks

```sh
mise run bench:dev          # fast dev run
mise run bench              # full run
mise run bench:plot         # regenerate plots/
```

See `mise.toml` for all available benchmark tasks.

## Pull requests

1. Fork the repo and create a branch from `main`.
2. Make your changes. Keep commits focused and conventional.
3. Ensure `cargo test --workspace` passes.
4. Ensure `cargo clippy --all-targets --all-features -- -D warnings` is clean.
5. Open a PR against `main`. Fill out the PR template.

### What we look for

- Tests for new functionality
- No new clippy warnings
- Consistent style with the rest of the codebase (rustfmt enforced)
- Benchmark results if touching transfer/download/upload hot paths

## Project structure

```text
crates/
  core/     # Library — S3 ops, auth, transfer engine
  cli/      # CLI binary wrapping core
  node/     # NAPI-RS bindings for Node.js
  python/   # PyO3 bindings for Python
bench/      # Benchmark harness (Python/uv)
examples/   # Example web servers (Rust, Python, TypeScript)
```

## License

By contributing, you agree that your contributions will be licensed under the
[MIT License](LICENSE).
