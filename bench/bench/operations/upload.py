"""Upload benchmark — measures tools across all S3 backends."""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from bench.infra import BACKENDS, start_backends, stop_backends
from bench.runner import bench_tool, discover_tools
from bench.types import OperationResult, UploadCmd

FILE_COUNT = int(os.environ.get("FILE_COUNT", "3"))
FILE_SIZE_MB = int(os.environ.get("FILE_SIZE_MB", "256"))
RUNS = int(os.environ.get("RUNS", "5"))
WORKERS = int(os.environ.get("WORKERS", "32"))
CONCURRENCY = int(os.environ.get("CONCURRENCY", "4"))
BUCKET = os.environ.get("BENCH_BUCKET", "bench-bucket")
PREFIX = "bench/"


def run() -> OperationResult:
    """Run the upload benchmark."""
    total_mb = FILE_COUNT * FILE_SIZE_MB
    config = {
        "files": FILE_COUNT,
        "total_mb": total_mb,
        "workers": WORKERS,
        "concurrency": CONCURRENCY,
    }

    print(f"  data:     {FILE_COUNT} x {FILE_SIZE_MB}MB = {total_mb}MB")
    print(f"  config:   {WORKERS}w x {CONCURRENCY}c")
    print(f"  runs:     {RUNS}")
    print()

    data_dir = Path(tempfile.mkdtemp(prefix="s3z_bench_"))
    try:
        _generate_data(data_dir)
        return _run_benchmarks(data_dir, config)
    finally:
        shutil.rmtree(data_dir, ignore_errors=True)


def _generate_data(data_dir: Path) -> None:
    print("  generating test data...")
    for i in range(1, FILE_COUNT + 1):
        subprocess.run(
            [
                "dd",
                "if=/dev/urandom",
                f"of={data_dir / f'file_{i}.bin'}",
                "bs=1M",
                f"count={FILE_SIZE_MB}",
            ],
            capture_output=True,
            check=True,
        )
    print("  test data ready.")


def _run_benchmarks(data_dir: Path, config: dict[str, int]) -> OperationResult:
    params = UploadCmd(
        data_dir=data_dir,
        bucket=BUCKET,
        prefix=PREFIX,
        workers=WORKERS,
        concurrency=CONCURRENCY,
    )

    start_backends()
    try:
        tools = discover_tools()
        for tool in tools:
            setup = getattr(tool, "setup", None)
            if callable(setup):
                setup(BACKENDS)
        print()

        result = OperationResult(operation="upload", config=config)

        print(f"  {'BACKEND':<10} {'TOOL':<8} min / mean / max")
        print(f"  {'-------':<10} {'----':<8} ----------------")

        for tool in tools:
            env_method = getattr(tool, "env", None)
            result.timings.extend(
                bench_tool(
                    name=tool.name,
                    cmd_fn=lambda b, t=tool: t.upload_cmd(b, params),
                    backends=BACKENDS,
                    bucket=BUCKET,
                    runs=RUNS,
                    env_fn=(lambda b, e=env_method: e(b)) if callable(env_method) else None,
                )
            )
            print()
    finally:
        stop_backends()

    return result
