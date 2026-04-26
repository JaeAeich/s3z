"""MinIO Client (mc) tool definition."""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from bench.types import Backend, DownloadCmd, ListCmd, UploadCmd


@dataclass
class McTool:
    name: str = "mc"

    def upload_cmd(self, backend: Backend, params: UploadCmd) -> list[str]:
        return [
            "mc",
            "cp",
            "--recursive",
            f"{params.data_dir}/",
            f"bench_{backend.name}/{params.bucket}/{params.prefix}",
        ]

    def download_cmd(self, backend: Backend, params: DownloadCmd) -> list[str]:
        return [
            "mc",
            "cp",
            "--recursive",
            f"bench_{backend.name}/{params.bucket}/{params.prefix}",
            str(params.dest_dir),
        ]

    def list_cmd(self, backend: Backend, params: ListCmd) -> list[str]:
        return [
            "mc",
            "ls",
            "--recursive",
            f"bench_{backend.name}/{params.bucket}/{params.prefix}",
        ]

    def env(self, backend: Backend) -> dict[str, str]:
        return {"AWS_REGION": backend.region, "AWS_DEFAULT_REGION": backend.region}

    def setup(self, backends: list[Backend]) -> None:
        access_key = os.environ["AWS_ACCESS_KEY_ID"]
        secret_key = os.environ["AWS_SECRET_ACCESS_KEY"]
        for backend in backends:
            subprocess.run(
                [
                    "mc",
                    "alias",
                    "set",
                    f"bench_{backend.name}",
                    backend.endpoint,
                    access_key,
                    secret_key,
                    "--api",
                    "S3v4",
                ],
                capture_output=True,
            )


tool = McTool()
