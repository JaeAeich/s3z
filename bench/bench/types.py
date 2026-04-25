"""Shared types for the benchmark framework."""

from __future__ import annotations

import csv
import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Protocol


@dataclass(frozen=True)
class Backend:
    """An S3-compatible backend to benchmark against."""

    name: str
    endpoint: str
    region: str


@dataclass(frozen=True)
class UploadCmd:
    """Parameters for building an upload command."""

    data_dir: Path
    bucket: str
    prefix: str
    workers: int
    concurrency: int


class Tool(Protocol):
    """A CLI tool that can be benchmarked.

    Required: name, upload_cmd.
    Optional: env (return extra env vars), setup (one-time init before runs).
    """

    name: str

    def upload_cmd(self, backend: Backend, params: UploadCmd) -> list[str]: ...


@dataclass
class TimingResult:
    """Timing result for a single tool+backend combination."""

    backend: str
    tool: str
    min_s: float
    mean_s: float
    max_s: float
    raw_times: list[float] = field(default_factory=list)


@dataclass
class OperationResult:
    """Result of benchmarking a single operation across all backends and tools."""

    operation: str
    config: dict[str, Any]
    timings: list[TimingResult] = field(default_factory=list)

    def to_csv(self, path: Path) -> None:
        """Write results to CSV."""
        with path.open("w", newline="") as f:
            writer = csv.writer(f)
            header = [
                "backend",
                "tool",
                *list(self.config.keys()),
                "runs",
                "min_s",
                "mean_s",
                "max_s",
                "raw_times",
            ]
            writer.writerow(header)
            for t in self.timings:
                row = [
                    t.backend,
                    t.tool,
                    *[str(self.config[k]) for k in self.config],
                    len(t.raw_times),
                    f"{t.min_s:.3f}",
                    f"{t.mean_s:.3f}",
                    f"{t.max_s:.3f}",
                    ";".join(f"{x:.3f}" for x in t.raw_times),
                ]
                writer.writerow(row)


@dataclass
class RunMeta:
    """Metadata about a benchmark run."""

    commit: str
    commit_short: str
    date: str
    os: str
    arch: str
    cpus: int
    memory_gb: float
    rust_version: str

    def to_json(self, path: Path) -> None:
        """Write metadata to JSON."""
        with path.open("w") as f:
            json.dump(self.__dict__, f, indent=2)

    @classmethod
    def from_json(cls, path: Path) -> RunMeta:
        """Load metadata from JSON."""
        with path.open() as f:
            data = json.load(f)
        return cls(**data)
