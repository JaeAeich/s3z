"""AWS CLI tool definition."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from bench.types import Backend, DownloadCmd, ListCmd, UploadCmd


@dataclass
class AwsTool:
    name: str = "aws"

    def download_cmd(self, backend: Backend, params: DownloadCmd) -> list[str]:
        return [
            "aws",
            "s3",
            "cp",
            f"s3://{params.bucket}/{params.prefix}",
            str(params.dest_dir),
            "--recursive",
            "--endpoint-url",
            backend.endpoint,
            "--region",
            backend.region,
            "--no-progress",
        ]

    def list_cmd(self, backend: Backend, params: ListCmd) -> list[str]:
        return [
            "aws",
            "s3api",
            "list-objects-v2",
            "--bucket",
            params.bucket,
            "--prefix",
            params.prefix,
            "--endpoint-url",
            backend.endpoint,
            "--region",
            backend.region,
            "--no-paginate",
            "--output",
            "json",
        ]

    def upload_cmd(self, backend: Backend, params: UploadCmd) -> list[str]:
        return [
            "aws",
            "s3",
            "cp",
            str(params.data_dir),
            f"s3://{params.bucket}/{params.prefix}",
            "--recursive",
            "--endpoint-url",
            backend.endpoint,
            "--region",
            backend.region,
            "--no-progress",
        ]


tool = AwsTool()
