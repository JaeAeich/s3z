"""Benchmark runner — discovers operations, executes them, collects results.

Design notes (see bench/README.md for the why):
- Samples are interleaved across (tool, backend) cells with a logged seed so
  thermal/drift bias spreads uniformly rather than landing on later tools.
- Sample count adapts: keep sampling a cell until 95% CI half-width drops below
  `target_rel_ci` of the mean, or `max_runs` is hit. `min_runs` is always run.
- Resource metrics (peak RSS, CPU user/sys) are collected via /usr/bin/time
  on macOS (-l) and Linux (-v). Failure to parse leaves them None.
"""

from __future__ import annotations

import importlib
import os
import platform
import random
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, cast

from bench import PROJECT_ROOT
from bench.infra import BACKENDS, start_backends, stop_backends
from bench.operations._api import default_cleanup, default_prepare, default_reset, tool_supports
from bench.types import Backend, OperationResult, RunMeta, Sample, TimingResult

if TYPE_CHECKING:
    from collections.abc import Callable

    from bench.operations._api import Op
    from bench.profile import Profile
    from bench.types import Tool


@dataclass(frozen=True)
class SamplingPolicy:
    """How many runs to take per cell. Adaptive within [min_runs, max_runs]."""

    min_runs: int
    max_runs: int
    target_rel_ci: float = 0.05  # stop once CI half-width <= 5% of mean

    @classmethod
    def fixed(cls, n: int) -> SamplingPolicy:
        return cls(min_runs=n, max_runs=n, target_rel_ci=0.0)


def collect_meta(profile: str = "full", seed: int = 0, interleave_seed: int = 0) -> RunMeta:
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
        profile=profile,
        seed=seed,
        interleave_seed=interleave_seed,
    )


def _discover_modules(package: str, attr: str) -> dict[str, Any]:
    """Discover modules in a subpackage and collect a named attribute from each."""
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


def filter_tools(tools: list[Tool], names: list[str] | None) -> list[Tool]:
    if not names:
        return tools
    by_name = {t.name: t for t in tools}
    missing = [n for n in names if n not in by_name]
    if missing:
        msg = f"unknown tool(s): {missing}; available: {list(by_name)}"
        raise ValueError(msg)
    return [by_name[n] for n in names]


def filter_backends(backends: list[Backend], names: list[str] | None) -> list[Backend]:
    if not names:
        return backends
    by_name = {b.name: b for b in backends}
    missing = [n for n in names if n not in by_name]
    if missing:
        msg = f"unknown backend(s): {missing}; available: {list(by_name)}"
        raise ValueError(msg)
    return [by_name[n] for n in names]


# --- Sample collection ----------------------------------------------------

_TIME_BIN = "/usr/bin/time"
_HAS_GTIME = shutil.which("gtime") is not None


def _time_wrapper() -> tuple[list[str], str] | None:
    """Pick a `time` binary that supports verbose output. Returns (argv, kind)."""
    if _HAS_GTIME:
        return (["gtime", "-v"], "gnu")
    if Path(_TIME_BIN).exists():
        if platform.system() == "Darwin":
            return ([_TIME_BIN, "-l"], "bsd")
        return ([_TIME_BIN, "-v"], "gnu")
    return None


def take_sample(cmd: list[str], env: dict[str, str] | None = None) -> Sample | None:
    """Run a command once, returning wall time + RSS + CPU. None on failure."""
    merged_env = {**os.environ, **(env or {})}
    wrapper = _time_wrapper()

    full_cmd = (wrapper[0] + cmd) if wrapper else cmd
    start = time.monotonic()
    result = subprocess.run(full_cmd, capture_output=True, env=merged_env)
    elapsed = time.monotonic() - start

    if result.returncode != 0:
        stderr = result.stderr.decode(errors="replace").strip() if result.stderr else ""
        if stderr:
            print(f"  stderr: {stderr[:200]}")
        return None

    sample = Sample(wall_s=round(elapsed, 3))
    if wrapper is not None:
        rss, cu, cs = _parse_time_output(result.stderr.decode(errors="replace"), wrapper[1])
        sample.rss_mb = rss
        sample.cpu_user_s = cu
        sample.cpu_sys_s = cs
    return sample


_BSD_RSS_RE = re.compile(r"\s*(\d+)\s+maximum resident set size")
_BSD_USER_RE = re.compile(r"\s*([\d.]+)\s+user")
_BSD_SYS_RE = re.compile(r"\s*([\d.]+)\s+sys")
_GNU_RSS_RE = re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")
_GNU_USER_RE = re.compile(r"User time \(seconds\):\s*([\d.]+)")
_GNU_SYS_RE = re.compile(r"System time \(seconds\):\s*([\d.]+)")


def _parse_time_output(text: str, kind: str) -> tuple[float | None, float | None, float | None]:
    """Parse RSS (MB) + user/sys CPU seconds from /usr/bin/time output."""
    rss_mb: float | None = None
    user_s: float | None = None
    sys_s: float | None = None
    if kind == "bsd":
        # macOS: maximum RSS reported in BYTES (despite man page wording).
        m = _BSD_RSS_RE.search(text)
        if m:
            rss_mb = int(m.group(1)) / (1024 * 1024)
        m = _BSD_USER_RE.search(text)
        if m:
            user_s = float(m.group(1))
        m = _BSD_SYS_RE.search(text)
        if m:
            sys_s = float(m.group(1))
    else:
        # GNU time: RSS in kbytes.
        m = _GNU_RSS_RE.search(text)
        if m:
            rss_mb = int(m.group(1)) / 1024
        m = _GNU_USER_RE.search(text)
        if m:
            user_s = float(m.group(1))
        m = _GNU_SYS_RE.search(text)
        if m:
            sys_s = float(m.group(1))

    # Sanity-check: RSS outside [0.5 MB, 100 GB] is almost certainly a unit mismatch.
    if rss_mb is not None and (rss_mb < 0.5 or rss_mb > 100_000):
        print(f"  WARN: suspicious RSS={rss_mb:.1f}MB — possible /usr/bin/time unit mismatch")
        rss_mb = None

    return rss_mb, user_s, sys_s


# --- Cell execution -------------------------------------------------------


@dataclass
class Cell:
    """A (tool, backend) pair to be sampled. Holds a TimingResult in progress.

    `reset_fn` is called before every sample (and during warmup). It does
    whatever the operation requires between runs — e.g. clear an upload bucket,
    or no-op for read-side ops where state stays put.
    """

    tool: Tool
    backend: Backend
    cmd_fn: Callable[[Backend], list[str]]
    env_fn: Callable[[Backend], dict[str, str] | None] | None
    reset_fn: Callable[[], None]
    result: TimingResult
    consecutive_failures: int = 0

    @property
    def label(self) -> str:
        return f"{self.backend.name}/{self.tool.name}"


_MAX_FAILURE_STREAK = 3


def run_interleaved(
    cells: list[Cell],
    policy: SamplingPolicy,
    interleave_seed: int,
) -> None:
    """Sample cells round-robin until each meets the policy.

    On each pass, all cells that still need samples are visited in a shuffled
    order (deterministic given the seed). This spreads thermal/load drift across
    cells uniformly rather than concentrating it in the last tool tested.
    """
    rng = random.Random(interleave_seed)  # noqa: S311  -- non-crypto, repro-only
    pending = list(cells)
    pass_idx = 0

    while pending:
        rng.shuffle(pending)
        pass_idx += 1
        next_pending: list[Cell] = []

        for cell in pending:
            if cell.result.n >= policy.max_runs:
                continue

            cell.reset_fn()
            env = cell.env_fn(cell.backend) if cell.env_fn else None
            sample = take_sample(cell.cmd_fn(cell.backend), env=env)

            if sample is None:
                cell.consecutive_failures += 1
                print(f"  FAIL {cell.label} (pass {pass_idx}, streak {cell.consecutive_failures})")
                if cell.consecutive_failures < _MAX_FAILURE_STREAK:
                    next_pending.append(cell)
                continue

            cell.consecutive_failures = 0
            cell.result.samples.append(sample)
            done = _cell_done(cell.result, policy)
            tag = " [done]" if done else ""
            print(
                f"  pass {pass_idx:>2}  {cell.label:<22} "
                f"n={cell.result.n:>2}  "
                f"mean={cell.result.mean_s:.3f}s  "
                f"±{cell.result.ci95_half:.3f}s  "
                f"({cell.result.rel_ci95 * 100:.1f}%){tag}",
            )
            sys.stdout.flush()

            if not done:
                next_pending.append(cell)

        pending = next_pending


def _cell_done(result: TimingResult, policy: SamplingPolicy) -> bool:
    if result.n < policy.min_runs:
        return False
    if result.n >= policy.max_runs:
        return True
    if policy.target_rel_ci <= 0:
        return False
    return result.rel_ci95 <= policy.target_rel_ci


def warmup_cells(cells: list[Cell], n: int = 1) -> None:
    """Run each cell n times, discarding results, to warm caches/connections."""
    if n <= 0:
        return
    print(f"  warmup: {n} run(s) per cell ({len(cells)} cells)")
    for cell in cells:
        for _ in range(n):
            cell.reset_fn()
            env = cell.env_fn(cell.backend) if cell.env_fn else None
            take_sample(cell.cmd_fn(cell.backend), env=env)


def build_cells(
    tools: list[Tool],
    backends: list[Backend],
    cmd_fn_for: Callable[[Tool], Callable[[Backend], list[str]]],
    reset_fn_for: Callable[[Tool, Backend], Callable[[], None]],
) -> list[Cell]:
    """Build one Cell per (tool, backend), each with its own command + reset fn."""
    cells: list[Cell] = []
    for tool in tools:
        env_method = getattr(tool, "env", None)
        env_fn = (lambda b, e=env_method: e(b)) if callable(env_method) else None
        cells.extend(
            Cell(
                tool=tool,
                backend=backend,
                cmd_fn=cmd_fn_for(tool),
                env_fn=env_fn,
                reset_fn=reset_fn_for(tool, backend),
                result=TimingResult(backend=backend.name, tool=tool.name),
            )
            for backend in backends
        )
    return cells


def _git(*args: str) -> str:
    return subprocess.check_output(["git", "-C", str(PROJECT_ROOT), *args], text=True)


# --- Operation orchestration ----------------------------------------------


def run_operation(
    op: Op,
    profile: Profile,
    *,
    interleave_seed: int = 0,
) -> OperationResult:
    """Drive one operation through its full lifecycle.

    Steps: filter tools/backends from the profile, start backends, build cells
    (with op-specific cmd/reset hooks), warm up, sample interleaved, tear down.

    Tools that don't implement `op.cmd_attr` are skipped with a warning.
    """
    backends = filter_backends(BACKENDS, list(profile.backends))
    all_tools = filter_tools(discover_tools(), list(profile.tools))
    tools = [t for t in all_tools if tool_supports(t, op)]
    skipped = [t.name for t in all_tools if t not in tools]
    if skipped:
        print(f"  skip (no {op.cmd_attr}): {skipped}")

    params = op.make_params(profile)
    config = op.csv_config(params)

    prepare = getattr(op, "prepare", default_prepare)
    reset = getattr(op, "reset", default_reset)
    cleanup = getattr(op, "cleanup", default_cleanup)

    policy = SamplingPolicy(
        min_runs=profile.min_runs,
        max_runs=profile.max_runs,
        target_rel_ci=profile.target_rel_ci,
    )

    start_backends(backends)
    try:
        for tool in tools:
            tool_setup = getattr(tool, "setup", None)
            if callable(tool_setup):
                tool_setup(backends)
        for backend in backends:
            prepare(backend, params)
        print()

        cells = build_cells(
            tools=tools,
            backends=backends,
            cmd_fn_for=lambda t: _cmd_fn_for(t, op, params),
            reset_fn_for=lambda _t, b: lambda b=b: reset(b, params),
        )

        warmup_cells(cells, n=profile.warmup_runs)
        print()
        run_interleaved(cells, policy, interleave_seed)

        for backend in backends:
            cleanup(backend, params)

        result = OperationResult(operation=op.name, config=config)
        result.timings = [c.result for c in cells if c.result.n > 0]
    finally:
        stop_backends()

    return result


def _cmd_fn_for(tool: Tool, op: Op, params: object) -> Callable[[Backend], list[str]]:
    """Bind a tool's per-op command builder to a single params object."""
    cmd_method = getattr(tool, op.cmd_attr)
    return lambda backend, _p=params: cmd_method(backend, _p)
