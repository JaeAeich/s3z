"""s5cmd tool definition."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from bench.types import Backend, DownloadCmd, ListCmd, UploadCmd


@dataclass
class S5cmdTool:
    name: str = "s5cmd"

    def upload_cmd(self, backend: Backend, params: UploadCmd) -> list[str]:
        cmd = [
            "s5cmd",
            "--endpoint-url",
            backend.endpoint,
        ]
        if params.workers > 0:
            cmd.extend(["--numworkers", str(params.workers)])
        cmd.extend(
            [
                "cp",
            ]
        )
        if params.concurrency > 0:
            cmd.extend(["--concurrency", str(params.concurrency)])
        cmd.extend(
            [
                f"{params.data_dir}/*",
                f"s3://{params.bucket}/{params.prefix}",
            ]
        )
        return cmd

    def download_cmd(self, backend: Backend, params: DownloadCmd) -> list[str]:
        return [
            "s5cmd",
            "--endpoint-url",
            backend.endpoint,
            "cp",
            f"s3://{params.bucket}/{params.prefix}*",
            str(params.dest_dir),
        ]

    def list_cmd(self, backend: Backend, params: ListCmd) -> list[str]:
        return [
            "s5cmd",
            "--endpoint-url",
            backend.endpoint,
            "ls",
            f"s3://{params.bucket}/{params.prefix}*",
        ]

    def env(self, backend: Backend) -> dict[str, str] | None:
        return {"AWS_REGION": backend.region}


tool = S5cmdTool()
