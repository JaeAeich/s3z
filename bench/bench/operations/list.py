"""List benchmark — measures ListObjectsV2 throughput and peak RSS.

Populates a bucket with N files, then benchmarks listing them all.
Data is uploaded once during prepare; each sample just lists.
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
from bench.types import ListCmd

if TYPE_CHECKING:
    from bench.profile import Profile
    from bench.types import Backend, OperationResult

BUCKET = os.environ.get("BENCH_BUCKET", "bench-bucket")
PREFIX = "list-bench/"

# Small files — we care about key count, not data volume.
FILE_SIZE_MB = 1


@dataclass
class ListOp:
    """The list operation. See bench/operations/_api.py for the contract."""

    name: str = "list"
    cmd_attr: str = "list_cmd"
    file_count: int = 0
    data_dir: Path | None = None

    def make_params(self, profile: Profile) -> ListCmd:
        """Upload test files into the bucket so there's something to list."""
        self.file_count = profile.file_count
        seed = int(os.environ.get("BENCH_SEED", "0"))
        self.data_dir = Path(tempfile.mkdtemp(prefix="s3z_list_bench_"))
        generate_test_data(self.data_dir, self.file_count, FILE_SIZE_MB, seed)
        return ListCmd(
            bucket=BUCKET,
            prefix=PREFIX,
            expected_keys=self.file_count,
        )

    def csv_config(self, params: ListCmd) -> dict[str, object]:
        return {
            "files": params.expected_keys,
            "file_size_mb": FILE_SIZE_MB,
            "total_mb": params.expected_keys * FILE_SIZE_MB,
        }

    def prepare(self, backend: Backend, _params: ListCmd) -> None:
        """Upload data into the bucket via s5cmd (backend-neutral)."""
        import subprocess

        env = {**os.environ, "AWS_REGION": backend.region}
        # Ensure bucket exists and is clean
        reset_bucket(backend, BUCKET)

        # Upload the generated files
        if self.data_dir is not None:
            subprocess.run(
                [
                    "s5cmd",
                    "--endpoint-url",
                    backend.endpoint,
                    "cp",
                    f"{self.data_dir}/*",
                    f"s3://{BUCKET}/{PREFIX}",
                ],
                capture_output=True,
                env=env,
                check=True,
            )
            print(f"  uploaded {self.file_count} files to {backend.name}/{BUCKET}/{PREFIX}")

    def cleanup(self, backend: Backend, _params: ListCmd) -> None:
        reset_bucket(backend, BUCKET)
        if self.data_dir is not None and self.data_dir.exists():
            shutil.rmtree(self.data_dir, ignore_errors=True)
            self.data_dir = None


def run(profile: Profile, *, interleave_seed: int = 0, data_seed: int = 0) -> OperationResult:
    """Entry point invoked by the discovery loop in cli.py."""
    os.environ["BENCH_SEED"] = str(data_seed)
    _print_header(profile, data_seed)
    return run_operation(ListOp(), profile, interleave_seed=interleave_seed)


def _print_header(profile: Profile, data_seed: int) -> None:
    print(
        f"  data:     {profile.file_count} x {FILE_SIZE_MB}MB = "
        f"{profile.file_count * FILE_SIZE_MB}MB  (seed={data_seed})",
    )
    print(
        f"  runs:     min={profile.min_runs} max={profile.max_runs} "
        f"target_rel_ci={profile.target_rel_ci:.0%}",
    )
    print(f"  backends: {list(profile.backends)}")
    print(f"  tools:    {list(profile.tools)}")
    print()
