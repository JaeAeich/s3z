"""CLI entry point for s3z-bench.

Subcommands:
  run        Run benchmarks (default: full profile; --dev for the fast profile).
  save       Persist the latest run. Full → benchmarks/ (committed).
            Dev  → target/bench/baseline/ (local, gitignored).
  plot       Generate charts from the latest run (local takes precedence).
  compare    Statistical regression check vs the local baseline.
  noise      Characterize the harness's measurement floor.
  sweep      Parameter sweeps (size, concurrency).
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import time
from pathlib import Path

from bench import PROJECT_ROOT
from bench.profile import get as get_profile
from bench.runner import collect_meta, discover_operations

BENCH_DIR = PROJECT_ROOT / "benchmarks"
LOCAL_BENCH_DIR = PROJECT_ROOT / "target" / "bench"
LOCAL_BASELINE_DIR = LOCAL_BENCH_DIR / "baseline"


def main() -> None:
    parser = argparse.ArgumentParser(prog="s3z-bench")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_run = sub.add_parser("run", help="Run benchmarks")
    p_run.add_argument("--dev", action="store_true", help="use the fast dev profile")
    p_run.add_argument("--backends", nargs="+", help="override profile backends")
    p_run.add_argument("--tools", nargs="+", help="override profile tools")
    p_run.add_argument("--files", type=int, help="override file count")
    p_run.add_argument("--file-size-mb", type=int, help="override file size (MB)")
    p_run.add_argument("--workers", type=int, help="override workers")
    p_run.add_argument("--concurrency", type=int, help="override concurrency")
    p_run.add_argument("--min-runs", type=int, help="override min runs")
    p_run.add_argument("--max-runs", type=int, help="override max runs")
    p_run.add_argument("--target-rel-ci", type=float, help="override target rel CI (e.g. 0.05)")
    p_run.add_argument("--seed", type=int, default=0, help="data PRNG seed")
    p_run.add_argument("--interleave-seed", type=int, default=0, help="run interleave seed")
    p_run.add_argument("ops", nargs="*", help="operations to run (default: all)")
    p_run.set_defaults(func=_cmd_run)

    p_save = sub.add_parser(
        "save",
        help="Persist latest run (full → benchmarks/, dev → baseline)",
    )
    p_save.set_defaults(func=_cmd_save)

    p_plot = sub.add_parser("plot", help="Generate charts from the latest run")
    p_plot.set_defaults(func=_cmd_plot)

    p_cmp = sub.add_parser("compare", help="Compare current local run vs baseline")
    p_cmp.add_argument("--baseline", help="override baseline run dir")
    p_cmp.add_argument("--current", help="override current run dir")
    p_cmp.add_argument("--op", default="upload", help="operation to compare")
    p_cmp.set_defaults(func=_cmd_compare)

    p_noise = sub.add_parser("noise", help="Characterize harness noise floor")
    p_noise.add_argument("--runs", type=int, default=60)
    p_noise.add_argument("--backend", default="minio")
    p_noise.add_argument("--tool", default="s3z")
    p_noise.add_argument("--files", type=int, default=3)
    p_noise.add_argument("--file-size-mb", type=int, default=64)
    p_noise.set_defaults(func=_cmd_noise)

    p_sweep = sub.add_parser("sweep", help="Parameter sweeps")
    sweep_sub = p_sweep.add_subparsers(dest="axis", required=True)

    p_sw_size = sweep_sub.add_parser("size", help="Sweep file size")
    p_sw_size.add_argument("--sizes-mb", nargs="+", type=int, default=[8, 32, 128, 512])
    p_sw_size.add_argument("--files", type=int, default=1)
    p_sw_size.add_argument("--backend", default="minio")
    p_sw_size.add_argument("--tools", nargs="+", default=["s3z", "s5cmd"])
    p_sw_size.add_argument("--workers", type=int, default=32)
    p_sw_size.add_argument("--concurrency", type=int, default=4)
    p_sw_size.set_defaults(func=_cmd_sweep_size)

    p_sw_conc = sweep_sub.add_parser("concurrency", help="Sweep workers × concurrency")
    p_sw_conc.add_argument(
        "--pairs",
        nargs="+",
        default=["4x1", "16x2", "32x4", "64x8"],
        help="space-separated WxC pairs",
    )
    p_sw_conc.add_argument("--backend", default="minio")
    p_sw_conc.add_argument("--tools", nargs="+", default=["s3z"])
    p_sw_conc.add_argument("--files", type=int, default=3)
    p_sw_conc.add_argument("--file-size-mb", type=int, default=128)
    p_sw_conc.set_defaults(func=_cmd_sweep_concurrency)

    # Mise's `{{arg(default='')}}` passes a literal '' when no args are given.
    # Drop those before argparse sees them.
    argv = [a for a in sys.argv[1:] if a != ""]
    args = parser.parse_args(argv)
    args.func(args)


def _cmd_run(args: argparse.Namespace) -> None:
    profile = _resolve_profile(args)
    meta = collect_meta(
        profile=profile.name,
        seed=args.seed,
        interleave_seed=args.interleave_seed,
    )
    run_id = _make_run_id(meta.commit_short, profile.name)
    run_dir = LOCAL_BENCH_DIR / run_id
    run_dir.mkdir(parents=True, exist_ok=True)
    meta.to_json(run_dir / "meta.json")

    print("\n=== s3z benchmark ===")
    print(f"  profile: {profile.name}")
    print(f"  commit:  {meta.commit_short}")
    print(f"  output:  target/bench/{run_id}/")
    print(f"  machine: {meta.os} {meta.arch} / {meta.cpus} cores / {meta.memory_gb} GB")
    print()

    available = discover_operations()
    ops = [o for o in (args.ops or []) if o] or list(available.keys())

    for op_name in ops:
        fn = available.get(op_name)
        if fn is None:
            print(f"ERROR: unknown operation '{op_name}'", file=sys.stderr)
            print(f"  available: {', '.join(available)}", file=sys.stderr)
            sys.exit(1)

        print(f"--- {op_name} ---")
        result = fn(profile, interleave_seed=args.interleave_seed, data_seed=args.seed)
        result.to_csv(run_dir / f"{op_name}.csv")
        print(f"  -> {run_id}/{op_name}.csv")
        print()

    latest = {"run_id": run_id, "commit": meta.commit_short, "profile": profile.name}
    (LOCAL_BENCH_DIR / "latest.json").write_text(json.dumps(latest, indent=2))
    print(f"=== Done: target/bench/{run_id}/ ===")


def _resolve_profile(args: argparse.Namespace):  # noqa: ANN202
    """Pick the profile (dev or full) and apply CLI overrides."""
    from dataclasses import replace

    base = get_profile("dev" if args.dev else "full")
    overrides: dict = {}
    if args.backends:
        overrides["backends"] = tuple(args.backends)
    if args.tools:
        overrides["tools"] = tuple(args.tools)
    if args.files is not None:
        overrides["file_count"] = args.files
    if args.file_size_mb is not None:
        overrides["file_size_mb"] = args.file_size_mb
    if args.workers is not None:
        overrides["workers"] = args.workers
    if args.concurrency is not None:
        overrides["concurrency"] = args.concurrency
    if args.min_runs is not None:
        overrides["min_runs"] = args.min_runs
    if args.max_runs is not None:
        overrides["max_runs"] = args.max_runs
    if args.target_rel_ci is not None:
        overrides["target_rel_ci"] = args.target_rel_ci
    return replace(base, **overrides) if overrides else base


def _cmd_save(_args: argparse.Namespace) -> None:
    """Save the latest run to the right place based on its profile."""
    local_latest = LOCAL_BENCH_DIR / "latest.json"
    if not local_latest.exists():
        print("ERROR: no local run found. Run 's3z-bench run' first.", file=sys.stderr)
        sys.exit(1)

    latest = json.loads(local_latest.read_text())
    run_id = latest["run_id"]
    src = LOCAL_BENCH_DIR / run_id
    profile = latest.get("profile", "full")

    if profile == "full":
        dst = BENCH_DIR / run_id
        dst.mkdir(parents=True, exist_ok=True)
        for f in src.iterdir():
            if f.suffix in {".json", ".csv"}:
                shutil.copy2(f, dst / f.name)
                print(f"  copied: {f.name}")
        pointer = {"run_id": run_id, "path": run_id, "commit": latest["commit"]}
        (BENCH_DIR / "latest.json").write_text(json.dumps(pointer, indent=2))
        print(f"\nSaved to benchmarks/{run_id}/  (commit with: git add benchmarks/)")
    else:
        if LOCAL_BASELINE_DIR.exists():
            shutil.rmtree(LOCAL_BASELINE_DIR)
        LOCAL_BASELINE_DIR.mkdir(parents=True)
        for f in src.iterdir():
            if f.suffix in {".json", ".csv"}:
                shutil.copy2(f, LOCAL_BASELINE_DIR / f.name)
        (LOCAL_BASELINE_DIR / "source.json").write_text(json.dumps(latest, indent=2))
        print(f"Baseline set: target/bench/baseline/  (from {run_id})")


def _cmd_plot(_args: argparse.Namespace) -> None:
    """Plot the latest run; prefer local, fall back to committed."""
    from bench.plot import plot_all

    if (LOCAL_BENCH_DIR / "latest.json").exists():
        plot_all(source="local")
    elif (BENCH_DIR / "latest.json").exists():
        plot_all(source="saved")
    else:
        print("ERROR: no run found in target/bench/ or benchmarks/.", file=sys.stderr)
        sys.exit(1)


def _cmd_compare(args: argparse.Namespace) -> None:
    from bench.compare import compare_runs, render

    base_dir = Path(args.baseline) if args.baseline else LOCAL_BASELINE_DIR
    if args.current:
        cur_dir = Path(args.current)
    else:
        latest = LOCAL_BENCH_DIR / "latest.json"
        if not latest.exists():
            print("ERROR: no local run found.", file=sys.stderr)
            sys.exit(1)
        cur_dir = LOCAL_BENCH_DIR / json.loads(latest.read_text())["run_id"]

    base_csv = base_dir / f"{args.op}.csv"
    cur_csv = cur_dir / f"{args.op}.csv"
    if not base_csv.exists():
        print(f"ERROR: baseline missing: {base_csv}", file=sys.stderr)
        print("       run 's3z-bench run --dev' then 's3z-bench save' first.", file=sys.stderr)
        sys.exit(1)
    if not cur_csv.exists():
        print(f"ERROR: current run missing: {cur_csv}", file=sys.stderr)
        sys.exit(1)

    print(f"comparing operation={args.op}: baseline={base_dir.name} vs current={cur_dir.name}")
    print()
    cmp = compare_runs(base_csv, cur_csv)
    print(render(cmp))
    n_regress = sum(1 for r in cmp.rows if r.verdict == "regress")
    sys.exit(1 if n_regress > 0 else 0)


def _cmd_noise(args: argparse.Namespace) -> None:
    from bench.noise import characterize

    characterize(
        runs=args.runs,
        backend_name=args.backend,
        tool_name=args.tool,
        file_count=args.files,
        file_size_mb=args.file_size_mb,
    )


def _cmd_sweep_size(args: argparse.Namespace) -> None:
    from bench.sweep import sweep_size

    out = LOCAL_BENCH_DIR / "sweeps" / f"size_{int(time.time())}.csv"
    sweep_size(
        sizes_mb=tuple(args.sizes_mb),
        file_count=args.files,
        backend_name=args.backend,
        tools=tuple(args.tools),
        workers=args.workers,
        concurrency=args.concurrency,
        out_path=out,
    )


def _cmd_sweep_concurrency(args: argparse.Namespace) -> None:
    from bench.sweep import sweep_concurrency

    pairs: list[tuple[int, int]] = []
    for s in args.pairs:
        try:
            w, c = s.lower().split("x")
            pairs.append((int(w), int(c)))
        except ValueError:
            print(f"ERROR: bad pair '{s}', expected WxC", file=sys.stderr)
            sys.exit(1)

    out = LOCAL_BENCH_DIR / "sweeps" / f"concurrency_{int(time.time())}.csv"
    sweep_concurrency(
        pairs=tuple(pairs),
        backend_name=args.backend,
        tools=tuple(args.tools),
        file_count=args.files,
        file_size_mb=args.file_size_mb,
        out_path=out,
    )


def _make_run_id(commit_short: str, profile: str) -> str:
    """Generate a sortable run ID: YYYY-MM-DD_HHMM_<profile>_<commit>."""
    ts = time.strftime("%Y-%m-%d_%H%M", time.gmtime())
    return f"{ts}_{profile}_{commit_short}"
