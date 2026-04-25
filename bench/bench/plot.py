"""Benchmark visualization — generates SVGs from saved benchmark results."""

from __future__ import annotations

import csv
import json
from dataclasses import dataclass
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
from matplotlib import ticker

from bench import PROJECT_ROOT
from bench.types import RunMeta

BENCH_DIR = PROJECT_ROOT / "benchmarks"
PLOTS_DIR = PROJECT_ROOT / "plots"

TOOL_COLORS: dict[str, str] = {
    "s3z": "#c8512a",
    "mc": "#1a1614",
    "s5cmd": "#5b8a72",
    "aws": "#3b7dd8",
}
TOOL_ORDER = ["s3z", "mc", "s5cmd", "aws"]
FALLBACK_COLORS = ["#e07b39", "#6c757d", "#28a745", "#17a2b8", "#ffc107", "#dc3545"]
BG = "#f3ede1"


@dataclass(frozen=True)
class PlotRow:
    """A single row of benchmark data for plotting."""

    backend: str
    tool: str
    min: float
    mean: float
    max: float
    total_mb: str
    runs: str


def load_latest() -> tuple[Path, RunMeta]:
    """Load the latest saved benchmark run."""
    latest_path = BENCH_DIR / "latest.json"
    if not latest_path.exists():
        msg = "No benchmarks/latest.json found. Run 'mise run bench:save' first."
        raise FileNotFoundError(msg)
    latest = json.loads(latest_path.read_text())
    run_dir = BENCH_DIR / str(latest["path"])
    meta = RunMeta.from_json(run_dir / "meta.json")
    return run_dir, meta


def plot_all() -> None:
    """Generate all available charts."""
    run_dir, meta = load_latest()
    print(f"Plotting benchmark: {meta.commit_short} ({meta.date})")

    PLOTS_DIR.mkdir(exist_ok=True)

    if (run_dir / "upload.csv").exists():
        _plot_upload(run_dir, meta)

    # Future operations: add more plotters here.
    # if (run_dir / "download.csv").exists():
    #     _plot_download(run_dir, meta)

    print("Done.")


def _plot_upload(run_dir: Path, meta: RunMeta) -> None:
    rows = _read_csv(run_dir / "upload.csv")
    if not rows:
        print("  skip: no valid upload data")
        return

    backends = list(dict.fromkeys(r.backend for r in rows))
    csv_tools = list(dict.fromkeys(r.tool for r in rows))
    tools = [t for t in TOOL_ORDER if t in csv_tools] + [
        t for t in csv_tools if t not in TOOL_ORDER
    ]
    total_mb = rows[0].total_mb
    runs = rows[0].runs
    fig, ax = _create_figure()
    _draw_grouped_bars(ax, rows, backends, tools)

    ax.set_title(
        f"s3z upload benchmark  //  {total_mb} MB, {runs} runs  //  {meta.commit_short}",
        fontsize=13,
        fontfamily="monospace",
        fontweight="bold",
        pad=16,
    )

    machine = f"{meta.os} {meta.arch} / {meta.cpus} cores / {meta.memory_gb} GB"
    fig.text(
        0.5,
        -0.02,
        machine,
        ha="center",
        fontsize=9,
        fontfamily="monospace",
        color="#1a1614",
        alpha=0.5,
    )

    plt.tight_layout()
    out = PLOTS_DIR / "upload.svg"
    fig.savefig(out, bbox_inches="tight", facecolor=BG)
    plt.close(fig)
    print(f"  saved: {out}")


def _read_csv(path: Path) -> list[PlotRow]:
    rows: list[PlotRow] = []
    with path.open() as f:
        for row in csv.DictReader(f):
            if float(row.get("mean_s", "0")) == 0:
                continue
            rows.append(
                PlotRow(
                    backend=row["backend"],
                    tool=row["tool"],
                    min=float(row["min_s"]),
                    mean=float(row["mean_s"]),
                    max=float(row["max_s"]),
                    total_mb=row.get("total_mb", "?") or "?",
                    runs=row.get("runs", "?") or "?",
                )
            )
    return rows


def _create_figure() -> tuple[plt.Figure, plt.Axes]:
    fig, ax = plt.subplots(figsize=(12, 6))
    fig.patch.set_facecolor(BG)
    ax.set_facecolor(BG)
    return fig, ax


def _draw_grouped_bars(
    ax: plt.Axes,
    rows: list[PlotRow],
    backends: list[str],
    tools: list[str],
) -> None:
    bar_width = 0.22
    group_gap = 0.15
    n_tools = len(tools)

    for ti, tool in enumerate(tools):
        xs: list[float] = []
        means: list[float] = []
        err_lo: list[float] = []
        err_hi: list[float] = []
        for bi, backend in enumerate(backends):
            matched = [r for r in rows if r.backend == backend and r.tool == tool]
            if not matched:
                continue
            r = matched[0]
            x = bi * (n_tools * bar_width + group_gap) + ti * bar_width
            xs.append(x)
            means.append(r.mean)
            err_lo.append(r.mean - r.min)
            err_hi.append(r.max - r.mean)

        ax.bar(
            xs,
            means,
            bar_width,
            color=TOOL_COLORS.get(tool, FALLBACK_COLORS[ti % len(FALLBACK_COLORS)]),
            label=tool,
            zorder=3,
            edgecolor="none",
        )
        ax.errorbar(
            xs,
            means,
            yerr=[err_lo, err_hi],
            fmt="none",
            ecolor="#1a1614",
            elinewidth=1.2,
            capsize=3,
            zorder=4,
        )

        for x, mean, hi in zip(xs, means, err_hi, strict=True):
            ax.text(
                x,
                mean + hi + 0.15,
                f"{mean:.2f}s",
                ha="center",
                va="bottom",
                fontsize=7.5,
                fontfamily="monospace",
                color="#1a1614",
                alpha=0.7,
            )

    centers = [
        bi * (n_tools * bar_width + group_gap) + (n_tools - 1) * bar_width / 2
        for bi in range(len(backends))
    ]
    ax.set_xticks(centers)
    ax.set_xticklabels(backends, fontfamily="monospace", fontsize=12, fontweight="bold")
    ax.set_ylabel("Time (seconds)", fontsize=12, fontfamily="monospace")
    ax.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.1f"))
    ax.grid(axis="y", alpha=0.3, zorder=0)
    ax.legend(
        loc="upper right",
        frameon=True,
        fancybox=False,
        edgecolor="#1a1614",
        fontsize=11,
        prop={"family": "monospace"},
    )
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)
    ax.spines["left"].set_color("#1a1614")
    ax.spines["bottom"].set_color("#1a1614")
    ax.tick_params(colors="#1a1614")
