"""AWS CLI tool definition."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from bench.types import Backend, UploadCmd


@dataclass
class AwsTool:
    name: str = "aws"

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
