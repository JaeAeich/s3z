"""Operation API — the contract every benchmarked operation implements.

A new operation is a single file in `bench/operations/` that defines an `Op`
subclass and exposes it as `run`. The harness handles everything else:
backend lifecycle, tool/backend filtering, warmup, interleaved sampling,
adaptive run counts, RSS/CPU collection, CSV emission.

What an op author writes:
  - `name`: unique identifier (matches the filename, e.g. "upload").
  - `cmd_attr`: which method to call on each Tool (e.g. "upload_cmd").
  - `make_params(profile)`: build the per-op params object from a Profile.
  - `csv_config(params)`: dict of fixed parameters logged to the CSV header.
  - `prepare(backend, params)`: one-time-per-cell setup (e.g. populate bucket
    for download). Optional; default is no-op.
  - `reset(backend, params)`: per-run reset between samples (e.g. clear
    bucket for upload). Default is no-op (override when needed).
  - `cleanup(backend, params)`: post-cell teardown. Default is no-op.

Tools that don't implement `cmd_attr` for a given op are skipped with a
warning — adding a new op without all tools supporting it is fine.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Protocol

if TYPE_CHECKING:
    from bench.profile import Profile
    from bench.types import Backend, Tool


class Op(Protocol):
    """Protocol every operation must satisfy.

    Implementations should be plain dataclasses or classes — the protocol is
    structural, no inheritance required. The params type is op-specific and
    deliberately untyped at this layer so a single `run_operation` can drive
    any operation; each op is responsible for matching its own params type
    across its lifecycle methods.
    """

    name: str
    cmd_attr: str  # method name on Tool that returns the command list

    def make_params(self, profile: Profile) -> Any: ...  # noqa: ANN401
    def csv_config(self, params: Any) -> dict[str, object]: ...  # noqa: ANN401

    # Optional lifecycle hooks; the runner uses `getattr(op, name, default_*)`
    # so they don't need to be declared on the Protocol.
    # def prepare(self, backend: Backend, params: Any) -> None: ...
    # def reset(self, backend: Backend, params: Any) -> None: ...
    # def cleanup(self, backend: Backend, params: Any) -> None: ...


def default_prepare(_backend: Backend, _params: object) -> None:
    """No-op default for ops that need no per-cell setup."""


def default_reset(_backend: Backend, _params: object) -> None:
    """No-op default for ops that need no per-run reset."""


def default_cleanup(_backend: Backend, _params: object) -> None:
    """No-op default for ops that need no post-cell teardown."""


def tool_supports(tool: Tool, op: Op) -> bool:
    """Whether `tool` implements `op.cmd_attr` (callable)."""
    fn = getattr(tool, op.cmd_attr, None)
    return callable(fn)
