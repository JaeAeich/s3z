"""FastAPI application with Scalar API documentation."""

from __future__ import annotations

from typing import TYPE_CHECKING

from fastapi import FastAPI
from scalar_fastapi import get_scalar_api_reference

from server.routes import router

if TYPE_CHECKING:
    from fastapi.responses import HTMLResponse

app = FastAPI(
    title="s3z",
    summary="S3 operations powered by s3z — fearlessly fast.",
    description=(
        "A thin REST layer over [s3z](https://github.com/jaeaeich/s3z)'s Python bindings.\n\n"
        "All heavy lifting (multipart upload/download, connection pooling, SigV4 signing) "
        "happens in the native Rust library; this server just exposes it over HTTP."
    ),
    version="0.1.0",
    docs_url=None,
    redoc_url=None,
)

app.include_router(router)


@app.get("/docs", include_in_schema=False)
async def scalar_docs() -> HTMLResponse:
    """Serve the Scalar API reference UI."""
    return get_scalar_api_reference(
        openapi_url=app.openapi_url or "/openapi.json",
        title=app.title,
    )
