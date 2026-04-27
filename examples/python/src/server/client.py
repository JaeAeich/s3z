"""S3 client singleton configured from environment variables."""

from __future__ import annotations

import os

from s3z import Config, CredentialSource, S3Client, static_credentials

REGION = os.environ.get("AWS_REGION", "us-east-1")
ENDPOINT = os.environ.get("S3_ENDPOINT")
BUCKET = os.environ.get("S3_BUCKET", "demo")

_access_key = os.environ.get("AWS_ACCESS_KEY_ID")
_secret_key = os.environ.get("AWS_SECRET_ACCESS_KEY")

creds = (
    static_credentials(_access_key, _secret_key)
    if _access_key and _secret_key
    else CredentialSource.Env
)

config = Config(REGION, credential_source=creds, endpoint=ENDPOINT)
client = S3Client(config)
