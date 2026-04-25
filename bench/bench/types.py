"""Shared types for the benchmark framework."""

from __future__ import annotations

import csv
import json
import math
import statistics
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


def quantile(values: list[float], q: float) -> float:
    """Linear-interpolated quantile. Returns 0.0 for empty input."""
    if not values:
        return 0.0
    s = sorted(values)
    if len(s) == 1:
        return s[0]
    k = q * (len(s) - 1)
    lo = math.floor(k)
    hi = math.ceil(k)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


class Tool(Protocol):
    """A CLI tool that can be benchmarked.

    Required: name, upload_cmd.
    Optional: env (return extra env vars), setup (one-time init before runs).
    """

    name: str

    def upload_cmd(self, backend: Backend, params: UploadCmd) -> list[str]: ...


@dataclass
class Sample:
    """A single timed run with optional resource metrics."""

    wall_s: float
    rss_mb: float | None = None
    cpu_user_s: float | None = None
    cpu_sys_s: float | None = None


@dataclass
class TimingResult:
    """Aggregated timing+resource result for a single tool+backend cell.

    Stores raw samples; statistics are computed lazily from them so that
    plotting and comparison code share a single source of truth.
    """

    backend: str
    tool: str
    samples: list[Sample] = field(default_factory=list)

    @property
    def wall_times(self) -> list[float]:
        return [s.wall_s for s in self.samples]

    @property
    def rss_values(self) -> list[float]:
        return [s.rss_mb for s in self.samples if s.rss_mb is not None]

    @property
    def n(self) -> int:
        return len(self.samples)

    @property
    def mean_s(self) -> float:
        return statistics.fmean(self.wall_times) if self.samples else 0.0

    @property
    def median_s(self) -> float:
        return statistics.median(self.wall_times) if self.samples else 0.0

    @property
    def stdev_s(self) -> float:
        return statistics.stdev(self.wall_times) if len(self.samples) >= 2 else 0.0

    @property
    def min_s(self) -> float:
        return min(self.wall_times) if self.samples else 0.0

    @property
    def max_s(self) -> float:
        return max(self.wall_times) if self.samples else 0.0

    @property
    def p95_s(self) -> float:
        return quantile(self.wall_times, 0.95)

    @property
    def ci95_half(self) -> float:
        """Half-width of the 95% CI of the mean (1.96 * sem)."""
        if self.n < 2:
            return 0.0
        return 1.96 * self.stdev_s / math.sqrt(self.n)

    @property
    def rel_ci95(self) -> float:
        """CI half-width as a fraction of the mean (e.g. 0.05 = ±5%)."""
        return (self.ci95_half / self.mean_s) if self.mean_s > 0 else 0.0

    @property
    def peak_rss_mb(self) -> float:
        return max(self.rss_values) if self.rss_values else 0.0

    @property
    def mean_rss_mb(self) -> float:
        return statistics.fmean(self.rss_values) if self.rss_values else 0.0


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
                "n",
                "mean_s",
                "median_s",
                "stdev_s",
                "ci95_half_s",
                "rel_ci95",
                "min_s",
                "max_s",
                "p95_s",
                "peak_rss_mb",
                "mean_rss_mb",
                "raw_times",
                "raw_rss_mb",
            ]
            writer.writerow(header)
            for t in self.timings:
                row = [
                    t.backend,
                    t.tool,
                    *[str(self.config[k]) for k in self.config],
                    t.n,
                    f"{t.mean_s:.3f}",
                    f"{t.median_s:.3f}",
                    f"{t.stdev_s:.3f}",
                    f"{t.ci95_half:.3f}",
                    f"{t.rel_ci95:.4f}",
                    f"{t.min_s:.3f}",
                    f"{t.max_s:.3f}",
                    f"{t.p95_s:.3f}",
                    f"{t.peak_rss_mb:.1f}",
                    f"{t.mean_rss_mb:.1f}",
                    ";".join(f"{x:.3f}" for x in t.wall_times),
                    ";".join(f"{x:.1f}" for x in t.rss_values),
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
    profile: str = "full"
    seed: int = 0
    interleave_seed: int = 0

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
