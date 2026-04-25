"""Noise-floor characterization — measure the harness's measurement floor.

Run the *same* binary, *same* backend, *same* workload many times back-to-back.
The resulting (relative stdev, absolute stdev) is the smallest change the
harness can reliably detect. Below that, regression alerts are noise.

Output: benchmarks/noise.json, consumed by compare.py.

Run once per machine; re-run when hardware/OS changes meaningfully.
"""

from __future__ import annotations

import json
import statistics
from dataclasses import replace

from bench import PROJECT_ROOT
from bench.operations.upload import UploadOp
from bench.profile import get as get_profile
from bench.runner import run_operation
from bench.types import quantile

DEFAULT_RUNS = 60
NOISE_PATH = PROJECT_ROOT / "benchmarks" / "noise.json"


def characterize(
    *,
    runs: int = DEFAULT_RUNS,
    backend_name: str = "minio",
    tool_name: str = "s3z",
    file_count: int = 3,
    file_size_mb: int = 64,
) -> dict[str, str | float]:
    """Run a single (backend, tool) cell N times and emit noise stats."""
    profile = replace(
        get_profile("dev"),
        backends=(backend_name,),
        tools=(tool_name,),
        file_count=file_count,
        file_size_mb=file_size_mb,
        min_runs=runs,
        max_runs=runs,
        target_rel_ci=0.0,
        warmup_runs=2,
    )

    print(f"  noise floor: {tool_name} on {backend_name}, n={runs}")
    print(f"  data:        {file_count} x {file_size_mb}MB")
    print()

    result = run_operation(UploadOp(), profile)
    if not result.timings:
        msg = "noise run produced no samples"
        raise RuntimeError(msg)

    stats = _compute_stats(result.timings[0].wall_times, backend_name, tool_name)
    _persist(stats)
    _report(stats)
    return stats


def _compute_stats(times: list[float], backend: str, tool: str) -> dict[str, str | float]:
    mean = statistics.fmean(times)
    sd = statistics.stdev(times) if len(times) >= 2 else 0.0
    p5 = quantile(times, 0.05)
    p95 = quantile(times, 0.95)
    return {
        "backend": backend,
        "tool": tool,
        "n": float(len(times)),
        "mean_s": round(mean, 4),
        "stdev_s": round(sd, 4),
        "rel": round(sd / mean if mean > 0 else 0.0, 4),
        "abs_s": round(sd, 4),
        "p5_s": round(p5, 4),
        "p95_s": round(p95, 4),
        "spread_pct": round((p95 - p5) / mean * 100 if mean > 0 else 0.0, 2),
    }


def _persist(stats: dict[str, str | float]) -> None:
    NOISE_PATH.parent.mkdir(parents=True, exist_ok=True)
    NOISE_PATH.write_text(json.dumps(stats, indent=2) + "\n")


def _report(stats: dict[str, str | float]) -> None:
    rel_pct = float(stats["rel"]) * 100
    print()
    print("  results:")
    print(f"    mean:      {stats['mean_s']:.3f}s")
    print(f"    stdev:     {stats['stdev_s']:.3f}s ({rel_pct:.1f}%)")
    print(f"    p5..p95:   {stats['p5_s']:.3f}s .. {stats['p95_s']:.3f}s")
    print(f"    spread:    {stats['spread_pct']:.1f}%")
    print()
    print(f"  saved: {NOISE_PATH.relative_to(PROJECT_ROOT)}")
    print(
        f"  → regression threshold: |Δmean| > max({stats['abs_s']:.3f}s, "
        f"{rel_pct:.1f}% × baseline)",
    )
