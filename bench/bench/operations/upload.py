"""Upload benchmark — measures sequential multipart upload throughput.

Each cell uploads a fresh batch of randomly-generated files into a clean
bucket prefix. Test data is generated once per `run()` invocation and shared
across all (tool, backend) cells; the bucket is reset before every sample.
"""

from __future__ import annotations

import os
import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from bench.data import generate_test_data
from bench.infra import reset_bucket
from bench.runner import run_operation
from bench.types import UploadCmd

if TYPE_CHECKING:
    from bench.profile import Profile
    from bench.types import Backend, OperationResult

BUCKET = os.environ.get("BENCH_BUCKET", "bench-bucket")
PREFIX = "bench/"


@dataclass
class UploadOp:
    """The upload operation. See bench/operations/_api.py for the contract."""

    name: str = "upload"
    cmd_attr: str = "upload_cmd"
    file_count: int = 0
    file_size_mb: int = 0
    data_dir: Path | None = None

    def make_params(self, profile: Profile) -> UploadCmd:
        """Generate the shared test data once, build the per-tool params."""
        self.file_count = profile.file_count
        self.file_size_mb = profile.file_size_mb
        seed = int(os.environ.get("BENCH_SEED", "0"))
        self.data_dir = Path(tempfile.mkdtemp(prefix="s3z_bench_"))
        generate_test_data(self.data_dir, profile.file_count, profile.file_size_mb, seed)
        return UploadCmd(
            data_dir=self.data_dir,
            bucket=BUCKET,
            prefix=PREFIX,
            workers=profile.workers,
            concurrency=profile.concurrency,
        )

    def csv_config(self, params: UploadCmd) -> dict[str, object]:
        return {
            "files": self.file_count,
            "file_size_mb": self.file_size_mb,
            "total_mb": self.file_count * self.file_size_mb,
            "workers": params.workers,
            "concurrency": params.concurrency,
        }

    def reset(self, backend: Backend, params: UploadCmd) -> None:
        reset_bucket(backend, params.bucket)

    def cleanup(self, _backend: Backend, _params: UploadCmd) -> None:
        if self.data_dir is not None and self.data_dir.exists():
            shutil.rmtree(self.data_dir, ignore_errors=True)
            self.data_dir = None


def run(profile: Profile, *, interleave_seed: int = 0, data_seed: int = 0) -> OperationResult:
    """Entry point invoked by the discovery loop in cli.py."""
    os.environ["BENCH_SEED"] = str(data_seed)
    _print_header(profile, data_seed)
    return run_operation(UploadOp(), profile, interleave_seed=interleave_seed)


def _print_header(profile: Profile, data_seed: int) -> None:
    total_mb = profile.file_count * profile.file_size_mb
    print(
        f"  data:     {profile.file_count} x {profile.file_size_mb}MB = {total_mb}MB  "
        f"(seed={data_seed})",
    )
    print(f"  config:   {profile.workers}w x {profile.concurrency}c")
    print(
        f"  runs:     min={profile.min_runs} max={profile.max_runs} "
        f"target_rel_ci={profile.target_rel_ci:.0%}",
    )
    print(f"  backends: {list(profile.backends)}")
    print(f"  tools:    {list(profile.tools)}")
    print()
