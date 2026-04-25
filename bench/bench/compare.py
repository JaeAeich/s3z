"""Regression detection — compare a current run against a baseline run.

Two-gate flagging: a cell regresses iff
  (a) Welch's t-test p < 0.05, AND
  (b) |Δmean| > max(noise_floor_abs, noise_floor_rel * baseline_mean).

The first gate guards against false positives from noisy cells that happen to
shift slightly between runs. The second guards against statistically significant
but practically meaningless shifts (e.g. a 0.5% mean change with n=200).

Noise floor is read from `benchmarks/noise.json` if present; otherwise a
conservative default (5% relative, 0.05s absolute) is used.
"""

from __future__ import annotations

import csv
import json
import math
from dataclasses import dataclass
from pathlib import Path

from bench import PROJECT_ROOT

DEFAULT_NOISE_REL = 0.05  # 5% — overridden by benchmarks/noise.json if present
DEFAULT_NOISE_ABS_S = 0.05
P_THRESHOLD = 0.05


@dataclass(frozen=True)
class CellStats:
    """Per-(backend, tool) summary parsed from a run's CSV."""

    backend: str
    tool: str
    n: int
    mean_s: float
    stdev_s: float
    rss_mb: float

    @property
    def variance(self) -> float:
        return self.stdev_s**2


@dataclass
class Comparison:
    operation: str
    rows: list[ComparisonRow]


@dataclass
class ComparisonRow:
    backend: str
    tool: str
    base: CellStats | None
    cur: CellStats | None
    delta_s: float
    delta_pct: float
    p_value: float
    rss_delta_mb: float
    verdict: str  # "ok", "regress", "improve", "new", "missing"


def load_run_csv(path: Path) -> list[CellStats]:
    rows: list[CellStats] = []
    with path.open() as f:
        for r in csv.DictReader(f):
            try:
                rows.append(
                    CellStats(
                        backend=r["backend"],
                        tool=r["tool"],
                        n=int(r["n"]),
                        mean_s=float(r["mean_s"]),
                        stdev_s=float(r.get("stdev_s") or 0.0),
                        rss_mb=float(r.get("peak_rss_mb") or 0.0),
                    )
                )
            except (KeyError, ValueError):
                continue
    return rows


def welch_p(a: CellStats, b: CellStats) -> float:
    """Welch's t-test p-value (two-sided), via normal approximation.

    For n>=8 (which our policy enforces) the t-distribution is close enough to
    normal that the approximation error is well below the noise floor. Avoids
    pulling scipy as a dependency.
    """
    if a.n < 2 or b.n < 2 or min(a.n, b.n) < 8:
        return 1.0  # Normal approximation unreliable below n=8
    se = math.sqrt(a.variance / a.n + b.variance / b.n)
    if se == 0:
        return 1.0 if a.mean_s == b.mean_s else 0.0
    t = (a.mean_s - b.mean_s) / se
    # Two-sided p from the standard normal CDF.
    return math.erfc(abs(t) / math.sqrt(2))


def load_noise_floor() -> tuple[float, float]:
    """Return (rel, abs_s). Prefer benchmarks/noise.json; fall back to defaults."""
    path = PROJECT_ROOT / "benchmarks" / "noise.json"
    if not path.exists():
        return DEFAULT_NOISE_REL, DEFAULT_NOISE_ABS_S
    try:
        data = json.loads(path.read_text())
        return float(data.get("rel", DEFAULT_NOISE_REL)), float(
            data.get("abs_s", DEFAULT_NOISE_ABS_S),
        )
    except (json.JSONDecodeError, ValueError):
        return DEFAULT_NOISE_REL, DEFAULT_NOISE_ABS_S


def compare_runs(baseline_csv: Path, current_csv: Path) -> Comparison:
    base = {(r.backend, r.tool): r for r in load_run_csv(baseline_csv)}
    cur = {(r.backend, r.tool): r for r in load_run_csv(current_csv)}
    keys = sorted(set(base) | set(cur))

    noise_rel, noise_abs = load_noise_floor()
    rows: list[ComparisonRow] = []
    for key in keys:
        b = base.get(key)
        c = cur.get(key)
        if b is None and c is not None:
            rows.append(
                ComparisonRow(key[0], key[1], None, c, 0.0, 0.0, 1.0, 0.0, "new"),
            )
            continue
        if c is None and b is not None:
            rows.append(
                ComparisonRow(key[0], key[1], b, None, 0.0, 0.0, 1.0, 0.0, "missing"),
            )
            continue
        assert b is not None and c is not None
        delta = c.mean_s - b.mean_s
        delta_pct = (delta / b.mean_s) if b.mean_s > 0 else 0.0
        p = welch_p(b, c)
        rss_delta = c.rss_mb - b.rss_mb

        threshold = max(noise_abs, noise_rel * b.mean_s)
        if p < P_THRESHOLD and abs(delta) > threshold:
            verdict = "regress" if delta > 0 else "improve"
        else:
            verdict = "ok"

        rows.append(
            ComparisonRow(key[0], key[1], b, c, delta, delta_pct, p, rss_delta, verdict),
        )

    op = current_csv.stem
    return Comparison(operation=op, rows=rows)


def render(cmp: Comparison) -> str:
    """Format a comparison as a readable table."""
    lines: list[str] = []
    lines.append(f"=== {cmp.operation} ===")
    lines.append(
        f"  {'BACKEND':<12} {'TOOL':<8} {'BASE':>9} {'CUR':>9} "
        f"{'Δ':>9} {'Δ%':>7} {'p':>7} {'ΔRSS':>9}  STATUS",
    )
    lines.append("  " + "-" * 90)
    icon = {"ok": " ", "regress": "↑", "improve": "↓", "new": "+", "missing": "-"}
    for r in cmp.rows:
        b_mean = f"{r.base.mean_s:>8.3f}s" if r.base else "       —"
        c_mean = f"{r.cur.mean_s:>8.3f}s" if r.cur else "       —"
        delta = f"{r.delta_s:>+7.3f}s" if r.base and r.cur else "       —"
        dpct = f"{r.delta_pct * 100:>+5.1f}%" if r.base and r.cur else "     —"
        p = f"{r.p_value:>7.3f}" if r.base and r.cur else "      —"
        rss = f"{r.rss_delta_mb:>+7.1f}MB" if r.base and r.cur else "       —"
        lines.append(
            f"  {r.backend:<12} {r.tool:<8} {b_mean} {c_mean} "
            f"{delta} {dpct} {p} {rss}  {icon[r.verdict]} {r.verdict}",
        )

    n_regress = sum(1 for r in cmp.rows if r.verdict == "regress")
    n_improve = sum(1 for r in cmp.rows if r.verdict == "improve")
    lines.append("")
    lines.append(f"  summary: {n_regress} regression(s), {n_improve} improvement(s)")
    return "\n".join(lines)
