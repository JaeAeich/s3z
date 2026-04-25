"""CLI entry point for s3z-bench."""

from __future__ import annotations

import json
import shutil
import sys
import time

from bench import PROJECT_ROOT
from bench.runner import collect_meta, discover_operations

BENCH_DIR = PROJECT_ROOT / "benchmarks"


def main() -> None:
    """Entry point for s3z-bench."""
    if len(sys.argv) < 2:
        _usage()
        return

    command = sys.argv[1]
    match command:
        case "run":
            _cmd_run(sys.argv[2:])
        case "save":
            _cmd_save()
        case "plot":
            _cmd_plot()
        case _:
            print(f"Unknown command: {command}", file=sys.stderr)
            _usage()
            sys.exit(1)


def _usage() -> None:
    print("Usage: s3z-bench <command>")
    print()
    print("Commands:")
    print("  run [op ...]   Run benchmarks (default: all operations)")
    print("  save           Copy latest run to benchmarks/ for committing")
    print("  plot           Generate charts from latest saved run")


def _cmd_run(ops: list[str]) -> None:
    meta = collect_meta()
    run_id = _make_run_id(meta.commit_short)
    run_dir = PROJECT_ROOT / "target" / "bench" / run_id
    run_dir.mkdir(parents=True, exist_ok=True)

    meta.to_json(run_dir / "meta.json")

    print("\n=== s3z benchmark ===")
    print(f"  commit:  {meta.commit_short}")
    print(f"  output:  target/bench/{run_id}/")
    print(f"  machine: {meta.os} {meta.arch} / {meta.cpus} cores / {meta.memory_gb} GB")
    print()

    available = discover_operations()
    if not ops:
        ops = list(available.keys())

    for op_name in ops:
        fn = available.get(op_name)
        if fn is None:
            print(f"ERROR: unknown operation '{op_name}'", file=sys.stderr)
            print(f"  available: {', '.join(available)}", file=sys.stderr)
            sys.exit(1)

        print(f"--- {op_name} ---")
        result = fn()
        result.to_csv(run_dir / f"{op_name}.csv")
        print(f"  -> {run_id}/{op_name}.csv")
        print()

    latest = {"run_id": run_id, "commit": meta.commit_short}
    (PROJECT_ROOT / "target" / "bench" / "latest.json").write_text(json.dumps(latest, indent=2))

    print(f"=== Done: target/bench/{run_id}/ ===")


def _cmd_save() -> None:
    local_latest = PROJECT_ROOT / "target" / "bench" / "latest.json"
    if not local_latest.exists():
        print("ERROR: no benchmark run found. Run 's3z-bench run' first.", file=sys.stderr)
        sys.exit(1)

    latest = json.loads(local_latest.read_text())
    run_id = latest["run_id"]
    src = PROJECT_ROOT / "target" / "bench" / run_id
    dst = BENCH_DIR / run_id

    dst.mkdir(parents=True, exist_ok=True)
    for f in src.iterdir():
        if f.suffix in {".json", ".csv"}:
            shutil.copy2(f, dst / f.name)
            print(f"  copied: {f.name}")

    pointer = {"run_id": run_id, "path": run_id, "commit": latest["commit"]}
    (BENCH_DIR / "latest.json").write_text(json.dumps(pointer, indent=2))
    print(f"\nSaved to benchmarks/{run_id}/")
    print("Ready to commit: git add benchmarks/")


def _cmd_plot() -> None:
    from bench.plot import plot_all

    plot_all()


def _make_run_id(commit_short: str) -> str:
    """Generate a sortable run ID: YYYY-MM-DD_HHMM_{commit}."""
    ts = time.strftime("%Y-%m-%d_%H%M", time.gmtime())
    return f"{ts}_{commit_short}"
