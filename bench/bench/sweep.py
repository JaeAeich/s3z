"""Parameter sweeps — concurrency × workers, file size.

Sweeps are opt-in (separate subcommand) because they are slow and only
informative when investigating a specific axis. Default `bench run` should
stay fast; this is what you reach for when you've changed multipart logic,
the worker pool, the read-ahead channel, etc.

Outputs a CSV per sweep with one row per (axis_value, tool, backend) cell.
Each sweep point reuses the standard run pipeline by synthesizing a one-off
Profile and driving it through `run_operation` — no parallel orchestration.
"""

from __future__ import annotations

import csv
from dataclasses import replace
from pathlib import Path
from typing import TYPE_CHECKING

from bench.operations.upload import UploadOp
from bench.profile import get as get_profile
from bench.runner import run_operation

if TYPE_CHECKING:
    from bench.profile import Profile
    from bench.types import TimingResult

# Defaults — keep small enough that a sweep finishes in single-digit minutes.
DEFAULT_SIZES_MB = (8, 32, 128, 512)
DEFAULT_CONCURRENCY = ((4, 1), (16, 2), (32, 4), (64, 8))  # (workers, concurrency)
SWEEP_MIN_RUNS = 5
SWEEP_MAX_RUNS = 15
SWEEP_REL_CI = 0.05


def sweep_size(
    *,
    sizes_mb: tuple[int, ...] = DEFAULT_SIZES_MB,
    file_count: int = 1,
    backend_name: str = "minio",
    tools: tuple[str, ...] = ("s3z", "s5cmd"),
    workers: int = 32,
    concurrency: int = 4,
    out_path: Path,
) -> None:
    """Sweep file size, holding tool/backend/concurrency fixed."""
    print(f"  sweep: size on {backend_name}, tools={list(tools)}")
    print(f"  sizes: {list(sizes_mb)} MB  ({file_count} file(s) per cell)")
    print(f"  fixed: {workers}w x {concurrency}c")
    print()

    base = _sweep_profile(backend_name, tools, file_count, workers, concurrency)
    rows: list[dict] = []
    for size_mb in sizes_mb:
        print(f"  --- size={size_mb}MB ---")
        profile = replace(base, file_size_mb=size_mb)
        result = run_operation(UploadOp(), profile, interleave_seed=size_mb)
        rows.extend(_size_row(t, size_mb, file_count, workers, concurrency) for t in result.timings)
        print()

    _write_sweep_csv(out_path, rows)


def sweep_concurrency(
    *,
    pairs: tuple[tuple[int, int], ...] = DEFAULT_CONCURRENCY,
    backend_name: str = "minio",
    tools: tuple[str, ...] = ("s3z",),
    file_count: int = 3,
    file_size_mb: int = 128,
    out_path: Path,
) -> None:
    """Sweep (workers, concurrency), holding file size fixed.

    Default tool list is `s3z` only — competitor tools' concurrency knobs do not
    map 1:1, so a multi-tool sweep on this axis would be misleading.
    """
    print(f"  sweep: concurrency on {backend_name}, tools={list(tools)}")
    print(f"  pairs: {list(pairs)}  (workers, concurrency)")
    print(f"  fixed: {file_count} x {file_size_mb}MB")
    print()

    rows: list[dict] = []
    for workers, concurrency in pairs:
        print(f"  --- {workers}w x {concurrency}c ---")
        profile = _sweep_profile(backend_name, tools, file_count, workers, concurrency)
        profile = replace(profile, file_size_mb=file_size_mb)
        result = run_operation(UploadOp(), profile, interleave_seed=workers * 1000 + concurrency)
        rows.extend(
            _concurrency_row(t, workers, concurrency, file_count, file_size_mb)
            for t in result.timings
        )
        print()

    _write_sweep_csv(out_path, rows)


def _sweep_profile(
    backend: str,
    tools: tuple[str, ...],
    file_count: int,
    workers: int,
    concurrency: int,
) -> Profile:
    """A small Profile suitable for one sweep cell."""
    return replace(
        get_profile("dev"),
        backends=(backend,),
        tools=tools,
        file_count=file_count,
        workers=workers,
        concurrency=concurrency,
        min_runs=SWEEP_MIN_RUNS,
        max_runs=SWEEP_MAX_RUNS,
        target_rel_ci=SWEEP_REL_CI,
        warmup_runs=1,
    )


def _size_row(
    t: TimingResult,
    size_mb: int,
    file_count: int,
    workers: int,
    concurrency: int,
) -> dict:
    return {
        "axis": "size_mb",
        "axis_value": size_mb,
        "backend": t.backend,
        "tool": t.tool,
        "workers": workers,
        "concurrency": concurrency,
        "files": file_count,
        **_stat_columns(t),
        "throughput_mb_s": _throughput(file_count * size_mb, t.mean_s),
    }


def _concurrency_row(
    t: TimingResult,
    workers: int,
    concurrency: int,
    file_count: int,
    file_size_mb: int,
) -> dict:
    return {
        "axis": "workers_x_concurrency",
        "axis_value": f"{workers}x{concurrency}",
        "workers": workers,
        "concurrency": concurrency,
        "backend": t.backend,
        "tool": t.tool,
        "files": file_count,
        "file_size_mb": file_size_mb,
        **_stat_columns(t),
        "throughput_mb_s": _throughput(file_count * file_size_mb, t.mean_s),
    }


def _stat_columns(t: TimingResult) -> dict:
    return {
        "n": t.n,
        "mean_s": round(t.mean_s, 3),
        "median_s": round(t.median_s, 3),
        "stdev_s": round(t.stdev_s, 3),
        "ci95_half_s": round(t.ci95_half, 3),
        "peak_rss_mb": round(t.peak_rss_mb, 1),
    }


def _throughput(total_mb: float, seconds: float) -> float:
    return round(total_mb / seconds, 1) if seconds > 0 else 0.0


def _write_sweep_csv(path: Path, rows: list[dict]) -> None:
    if not rows:
        print("  no rows to write")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)
    print(f"  saved: {path}")
