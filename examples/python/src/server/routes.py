"""API route handlers for S3 operations."""

from __future__ import annotations

import shutil
import tempfile
from pathlib import Path
from typing import Annotated

from fastapi import APIRouter, HTTPException, Query, UploadFile
from fastapi.responses import FileResponse

from server.client import BUCKET, client
from server.models import ErrorDetail, ObjectInfo, UploadFileResult

router = APIRouter(tags=["s3"])


@router.post(
    "/upload",
    response_model=list[UploadFileResult],
    summary="Upload files",
    responses={500: {"model": ErrorDetail, "description": "S3 or I/O error."}},
)
async def upload(
    files: list[UploadFile],
    prefix: Annotated[
        str,
        Query(description="Key prefix prepended to each uploaded file name."),
    ] = "",
) -> list[UploadFileResult]:
    """Upload one or more files to S3.

    Files are staged to a temporary directory, uploaded via s3z's multipart
    engine, then cleaned up. Large files are automatically split into
    concurrent parts.
    """
    staging = Path(tempfile.mkdtemp())
    try:
        paths: list[str] = []
        for f in files:
            dest = staging / (f.filename or "unnamed")
            dest.write_bytes(await f.read())
            paths.append(str(dest))

        results = client.upload(paths, BUCKET, prefix)
        return [
            UploadFileResult(key=r.key, size=r.size, parts=r.parts, etag=r.etag) for r in results
        ]
    except Exception as exc:
        raise HTTPException(status_code=500, detail=str(exc)) from exc
    finally:
        shutil.rmtree(staging, ignore_errors=True)


@router.get(
    "/download",
    summary="Download a file",
    response_class=FileResponse,
    responses={
        200: {"content": {"application/octet-stream": {}}, "description": "File contents."},
        404: {"model": ErrorDetail, "description": "Object not found."},
        500: {"model": ErrorDetail, "description": "S3 or I/O error."},
    },
)
async def download(
    key: Annotated[str, Query(description="Full S3 object key to download.")],
) -> FileResponse:
    """Download a single object from S3 by its full key.

    The prefix is derived from the key (everything up to the last `/`),
    then s3z downloads the matching prefix and returns the exact key match.
    """
    dest_dir = Path(tempfile.mkdtemp())
    slash = key.rfind("/")
    prefix = key[: slash + 1] if slash >= 0 else ""

    try:
        results = client.download(BUCKET, prefix, str(dest_dir))
    except Exception as exc:
        raise HTTPException(status_code=500, detail=str(exc)) from exc

    for r in results:
        if r.key == key:
            return FileResponse(r.dest, filename=Path(r.dest).name)

    raise HTTPException(status_code=404, detail=f"key not found: {key}")


@router.get(
    "/list",
    response_model=list[ObjectInfo],
    summary="List objects",
    responses={500: {"model": ErrorDetail, "description": "S3 or I/O error."}},
)
async def list_objects(
    prefix: Annotated[
        str,
        Query(description="Key prefix to filter by (e.g. `data/2024/`)."),
    ] = "",
    delimiter: Annotated[
        str | None,
        Query(description="Delimiter for directory-style grouping (typically `/`)."),
    ] = None,
) -> list[ObjectInfo]:
    """List objects under a prefix.

    Returns metadata for every object whose key starts with `prefix`.
    Pass `delimiter=/` to get a directory-like listing.
    """
    try:
        objects = client.list(BUCKET, prefix, delimiter)
    except Exception as exc:
        raise HTTPException(status_code=500, detail=str(exc)) from exc

    return [
        ObjectInfo(
            key=o.key,
            size=o.size,
            etag=o.etag,
            last_modified=o.last_modified,
        )
        for o in objects
    ]
