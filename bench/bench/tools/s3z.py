"""s3z tool definition."""

from __future__ import annotations

import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

from bench import PROJECT_ROOT

if TYPE_CHECKING:
    from bench.types import Backend, UploadCmd


def _build_s3z() -> Path:
    """Build s3z in release mode, return binary path."""
    print("  building s3z (release)...")
    subprocess.run(
        ["cargo", "build", "--release", "--bin", "s3z", "--quiet"],
        cwd=str(PROJECT_ROOT),
        check=True,
    )
    return PROJECT_ROOT / "target" / "release" / "s3z"


@dataclass
class S3zTool:
    name: str = "s3z"
    _bin: Path | None = field(default=None, repr=False)

    def upload_cmd(self, backend: Backend, params: UploadCmd) -> list[str]:
        binary = self._ensure_built()
        return [
            str(binary),
            "-e",
            backend.endpoint,
            "-r",
            backend.region,
            "upload",
            str(params.data_dir),
            "-b",
            params.bucket,
            "-p",
            params.prefix,
            "-w",
            str(params.workers),
            "-c",
            str(params.concurrency),
            "-q",
        ]

    def setup(self, _backends: list[Backend]) -> None:
        self._bin = _build_s3z()

    def _ensure_built(self) -> Path:
        if self._bin is None:
            self._bin = _build_s3z()
        return self._bin


tool = S3zTool()
