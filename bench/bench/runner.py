"""Benchmark runner — discovers operations, executes them, collects results."""

from __future__ import annotations

import importlib
import os
import platform
import subprocess
import time
from pathlib import Path
from typing import TYPE_CHECKING, Any, cast

from bench import PROJECT_ROOT
from bench.infra import reset_bucket
from bench.types import Backend, RunMeta, TimingResult

if TYPE_CHECKING:
    from collections.abc import Callable

    from bench.types import OperationResult, Tool


def collect_meta() -> RunMeta:
    """Gather machine and git metadata."""
    commit_full = _git("rev-parse", "HEAD").strip()
    commit_short = _git("rev-parse", "--short", "HEAD").strip()
    rust_version = subprocess.check_output(["rustc", "--version"], text=True).strip()

    mem_gb: float = 0.0
    try:
        mem = subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True).strip()
        mem_gb = round(int(mem) / (1024**3), 1)
    except (subprocess.CalledProcessError, FileNotFoundError):
        try:
            with Path("/proc/meminfo").open() as f:
                for line in f:
                    if line.startswith("MemTotal"):
                        mem_gb = round(int(line.split()[1]) / (1024**2), 1)
                        break
        except FileNotFoundError:
            pass

    return RunMeta(
        commit=commit_full,
        commit_short=commit_short,
        date=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        os=platform.system(),
        arch=platform.machine(),
        cpus=os.cpu_count() or 0,
        memory_gb=mem_gb,
        rust_version=rust_version,
    )


def _discover_modules(package: str, attr: str) -> dict[str, Any]:
    """Discover modules in a subpackage and collect a named attribute from each.

    Returns a dict keyed by module stem (e.g. "upload", "s3z").
    """
    results: dict[str, Any] = {}
    pkg_dir = Path(__file__).parent / package
    for path in sorted(pkg_dir.glob("*.py")):
        if path.name.startswith("_"):
            continue
        module = importlib.import_module(f"bench.{package}.{path.stem}")
        value = getattr(module, attr, None)
        if value is not None:
            results[path.stem] = value
    return results


def discover_operations() -> dict[str, Callable[..., OperationResult]]:
    """Discover benchmark operations from bench.operations package."""
    return cast("dict[str, Callable[..., OperationResult]]", _discover_modules("operations", "run"))


def discover_tools() -> list[Tool]:
    """Discover benchmark tools from bench.tools package."""
    return cast("list[Tool]", list(_discover_modules("tools", "tool").values()))


def time_command(cmd: list[str], env: dict[str, str] | None = None) -> float | None:
    """Run a command and return wall-clock seconds, or None on failure."""
    merged_env = {**os.environ, **(env or {})}
    start = time.monotonic()
    result = subprocess.run(cmd, capture_output=True, env=merged_env)
    elapsed = time.monotonic() - start
    if result.returncode != 0:
        stderr = result.stderr.decode(errors="replace").strip() if result.stderr else ""
        if stderr:
            print(f"  stderr: {stderr[:200]}")
        return None
    return round(elapsed, 3)


def bench_tool(
    name: str,
    cmd_fn: Callable[[Backend], list[str]],
    backends: list[Backend],
    bucket: str,
    runs: int,
    env_fn: Callable[[Backend], dict[str, str] | None] | None = None,
) -> list[TimingResult]:
    """Benchmark a single tool across all backends."""
    results: list[TimingResult] = []
    for backend in backends:
        print(f"  {backend.name:<10} {name:<8} ", end="", flush=True)
        # warmup run (discarded) to avoid cold-start noise
        reset_bucket(backend, bucket)
        env = env_fn(backend) if env_fn else None
        time_command(cmd_fn(backend), env=env)
        times: list[float] = []
        for _ in range(runs):
            reset_bucket(backend, bucket)
            env = env_fn(backend) if env_fn else None
            elapsed = time_command(cmd_fn(backend), env=env)
            if elapsed is None:
                print("FAIL")
                break
            times.append(elapsed)
        else:
            result = TimingResult(
                backend=backend.name,
                tool=name,
                min_s=min(times),
                mean_s=round(sum(times) / len(times), 3),
                max_s=max(times),
                raw_times=times,
            )
            results.append(result)
            print(f"{result.min_s:.3f},{result.mean_s:.3f},{result.max_s:.3f}")
    return results


def _git(*args: str) -> str:
    return subprocess.check_output(["git", "-C", str(PROJECT_ROOT), *args], text=True)
