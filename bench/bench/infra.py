"""Docker Compose lifecycle and S3 bucket management."""

from __future__ import annotations

import json
import os
import subprocess
import time

from bench import PROJECT_ROOT
from bench.types import Backend

COMPOSE_FILE = PROJECT_ROOT / "docker-compose.yaml"

BACKENDS = [
    Backend("minio", "http://localhost:9000", "us-east-1"),
    Backend("rustfs", "http://localhost:9300", "us-east-1"),
    Backend("seaweedfs", "http://localhost:9500", "us-east-1"),
    Backend("garage", "http://localhost:9700", "garage"),
]


def start_backends(backends: list[Backend] | None = None) -> None:
    """Start the requested S3 backends via docker compose.

    If `backends` is None, brings up all four. Otherwise brings up only the
    named subset — saves cold-start time during dev runs that target one
    backend.
    """
    targets = backends if backends is not None else BACKENDS
    target_names = [b.name for b in targets]
    print(f"  starting backends: {target_names}")
    cmd = ["docker", "compose", "-f", str(COMPOSE_FILE)]
    for name in target_names:
        cmd.extend(["--profile", name])
    cmd.extend(["up", "-d"])
    subprocess.run(cmd, check=True, capture_output=True)

    print("  waiting for backends...")
    for backend in targets:
        _wait_healthy(backend.name)
    time.sleep(3)
    print("  all backends healthy.")


def stop_backends() -> None:
    """Stop all backends and remove volumes."""
    print("  stopping backends...")
    subprocess.run(
        [
            "docker",
            "compose",
            "-f",
            str(COMPOSE_FILE),
            "--profile",
            "all",
            "down",
            "-v",
            "--remove-orphans",
        ],
        capture_output=True,
    )


def reset_bucket(backend: Backend, bucket: str) -> None:
    """Ensure a clean bucket exists: create if missing, then remove all objects."""
    env = {**os.environ, "AWS_REGION": backend.region}
    subprocess.run(
        ["s5cmd", "--endpoint-url", backend.endpoint, "mb", f"s3://{bucket}"],
        capture_output=True,
        env=env,
    )
    subprocess.run(
        ["s5cmd", "--endpoint-url", backend.endpoint, "rm", f"s3://{bucket}/*"],
        capture_output=True,
        env=env,
    )


def _wait_healthy(service: str, timeout: int = 60) -> None:
    """Wait for a docker compose service to report healthy."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = subprocess.run(
            [
                "docker",
                "compose",
                "-f",
                str(COMPOSE_FILE),
                "--profile",
                "all",
                "ps",
                service,
                "--format",
                "json",
            ],
            capture_output=True,
            text=True,
        )
        if result.returncode == 0 and result.stdout.strip():
            try:
                first_line = result.stdout.strip().split("\n")[0]
                data = json.loads(first_line)
                if data.get("Health") == "healthy":
                    return
            except (json.JSONDecodeError, KeyError):
                pass
        time.sleep(1)
    msg = f"Timeout waiting for {service} to become healthy"
    raise TimeoutError(msg)
