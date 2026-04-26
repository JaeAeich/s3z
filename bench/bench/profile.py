"""Benchmark profiles — predefined (tools, backends, file size, runs) bundles.

Two canonical profiles:

- `dev`: fast inner loop. Subset of backends/tools, smaller files, tighter run cap.
  Aimed at "I made a change, did I regress?" feedback in 2-5 min.

- `full`: canonical reference run. All backends, all tools, larger files, higher
  run cap. The numbers committed under benchmarks/. ~10-15 min.

Profiles are *defaults*. CLI flags (--backends, --tools, --files,
--file-size-mb, --workers, --concurrency, --min-runs, --max-runs,
--target-rel-ci) override individual fields.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Profile:
    name: str
    backends: tuple[str, ...]
    tools: tuple[str, ...]
    file_count: int
    file_size_mb: int
    workers: int
    concurrency: int
    min_runs: int
    max_runs: int
    target_rel_ci: float
    warmup_runs: int


PROFILES: dict[str, Profile] = {
    "dev": Profile(
        name="dev",
        backends=("minio", "rustfs", "seaweedfs", "garage"),
        tools=("s3z", "mc"),
        file_count=3,
        file_size_mb=128,
        workers=32,
        concurrency=8,
        min_runs=8,
        max_runs=30,
        target_rel_ci=0.05,
        warmup_runs=1,
    ),
    "full": Profile(
        name="full",
        backends=("minio", "rustfs", "seaweedfs", "garage"),
        tools=("s3z", "mc", "s5cmd", "aws"),
        file_count=3,
        file_size_mb=256,
        workers=32,
        concurrency=4,
        min_runs=10,
        max_runs=30,
        target_rel_ci=0.05,
        warmup_runs=1,
    ),
}


def get(name: str) -> Profile:
    if name not in PROFILES:
        msg = f"unknown profile '{name}'; available: {list(PROFILES)}"
        raise ValueError(msg)
    return PROFILES[name]
