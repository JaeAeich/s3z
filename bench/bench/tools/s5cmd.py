"""s5cmd tool definition."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from bench.types import Backend, UploadCmd


@dataclass
class S5cmdTool:
    name: str = "s5cmd"

    def upload_cmd(self, backend: Backend, params: UploadCmd) -> list[str]:
        return [
            "s5cmd",
            "--endpoint-url",
            backend.endpoint,
            "--numworkers",
            str(params.workers),
            "cp",
            "--concurrency",
            str(params.concurrency),
            f"{params.data_dir}/*",
            f"s3://{params.bucket}/{params.prefix}",
        ]

    def env(self, backend: Backend) -> dict[str, str] | None:
        return {"AWS_REGION": backend.region}


tool = S5cmdTool()
