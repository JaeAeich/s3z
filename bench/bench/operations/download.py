"""Download benchmark — measures batch download throughput and peak RSS.

Populates a bucket with N files during prepare, then benchmarks downloading
them all to a local temp directory. The bucket stays populated between samples;
only the local destination is cleared on each reset.
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
from bench.types import DownloadCmd

if TYPE_CHECKING:
    from bench.profile import Profile
    from bench.types import Backend, OperationResult

BUCKET = os.environ.get("BENCH_BUCKET", "bench-bucket")
PREFIX = "download-bench/"


@dataclass
class DownloadOp:
    """The download operation. See bench/operations/_api.py for the contract."""

    name: str = "download"
    cmd_attr: str = "download_cmd"
    file_count: int = 0
    file_size_mb: int = 0
    data_dir: Path | None = None
    dest_dir: Path | None = None

    def make_params(self, profile: Profile) -> DownloadCmd:
        """Upload test files so there's something to download."""
        self.file_count = profile.file_count
        self.file_size_mb = profile.file_size_mb
        seed = int(os.environ.get("BENCH_SEED", "0"))
        self.data_dir = Path(tempfile.mkdtemp(prefix="s3z_dl_src_"))
        self.dest_dir = Path(tempfile.mkdtemp(prefix="s3z_dl_dest_"))
        generate_test_data(self.data_dir, profile.file_count, profile.file_size_mb, seed)
        return DownloadCmd(
            bucket=BUCKET,
            prefix=PREFIX,
            dest_dir=self.dest_dir,
        )

    def csv_config(self, params: DownloadCmd) -> dict[str, object]:  # noqa: ARG002
        return {
            "files": self.file_count,
            "file_size_mb": self.file_size_mb,
            "total_mb": self.file_count * self.file_size_mb,
        }

    def prepare(self, backend: Backend, _params: DownloadCmd) -> None:
        """Upload data into the bucket via s5cmd (backend-neutral)."""
        import subprocess

        env = {**os.environ, "AWS_REGION": backend.region}
        reset_bucket(backend, BUCKET)

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

    def reset(self, _backend: Backend, params: DownloadCmd) -> None:
        """Clear the local destination between samples."""
        if params.dest_dir.exists():
            shutil.rmtree(params.dest_dir, ignore_errors=True)
            params.dest_dir.mkdir(parents=True, exist_ok=True)

    def cleanup(self, backend: Backend, _params: DownloadCmd) -> None:
        reset_bucket(backend, BUCKET)
        if self.data_dir is not None and self.data_dir.exists():
            shutil.rmtree(self.data_dir, ignore_errors=True)
            self.data_dir = None
        if self.dest_dir is not None and self.dest_dir.exists():
            shutil.rmtree(self.dest_dir, ignore_errors=True)
            self.dest_dir = None


def run(profile: Profile, *, interleave_seed: int = 0, data_seed: int = 0) -> OperationResult:
    """Entry point invoked by the discovery loop in cli.py."""
    os.environ["BENCH_SEED"] = str(data_seed)
    _print_header(profile, data_seed)
    return run_operation(DownloadOp(), profile, interleave_seed=interleave_seed)


def _print_header(profile: Profile, data_seed: int) -> None:
    total_mb = profile.file_count * profile.file_size_mb
    print(
        f"  data:     {profile.file_count} x {profile.file_size_mb}MB = {total_mb}MB  "
        f"(seed={data_seed})",
    )
    print(
        f"  runs:     min={profile.min_runs} max={profile.max_runs} "
        f"target_rel_ci={profile.target_rel_ci:.0%}",
    )
    print(f"  backends: {list(profile.backends)}")
    print(f"  tools:    {list(profile.tools)}")
    print()
