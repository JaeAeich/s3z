"""Test data generation for benchmarks.

Random data prevents storage-side dedup/compression from biasing results.
Seeding makes the byte stream deterministic across sessions.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np

_CHUNK = 1 << 20  # 1 MiB


def generate_test_data(data_dir: Path, file_count: int, file_size_mb: int, seed: int) -> None:
    """Generate `file_count` files of `file_size_mb` MiB using a seeded PRNG."""
    print("  generating test data...")
    rng = np.random.default_rng(seed)
    size_bytes = file_size_mb * (1 << 20)
    for i in range(1, file_count + 1):
        path = data_dir / f"file_{i}.bin"
        with path.open("wb") as f:
            remaining = size_bytes
            while remaining > 0:
                n = min(_CHUNK, remaining)
                f.write(rng.bytes(n))
                remaining -= n
    print("  test data ready.")
