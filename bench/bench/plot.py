"""Benchmark visualization — generates SVGs from saved benchmark results.

Charts show mean ± 95% CI half-width (not min/max — those grow with sample
count and mislead). A second panel renders peak RSS per cell so memory
regressions are visible alongside wall-time. The chart footer documents which
tools used which concurrency knobs (parity is not always achievable).
"""

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
LOCAL_BENCH_DIR = PROJECT_ROOT / "target" / "bench"
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
    """A single (backend, tool) cell, parsed from CSV."""

    backend: str
    tool: str
    n: int
    mean: float
    median: float
    stdev: float
    ci95_half: float
    rss_mb: float
    total_mb: str
    workers: str = "?"
    concurrency: str = "?"


def load_run(source: str = "saved") -> tuple[Path, RunMeta]:
    """Load a benchmark run.

    source = "saved" → benchmarks/latest.json (canonical full runs).
    source = "local" → target/bench/latest.json (most recent local run).
    """
    if source == "saved":
        latest_path = BENCH_DIR / "latest.json"
        root = BENCH_DIR
    elif source == "local":
        latest_path = LOCAL_BENCH_DIR / "latest.json"
        root = LOCAL_BENCH_DIR
    else:
        msg = f"unknown source: {source}"
        raise ValueError(msg)

    if not latest_path.exists():
        msg = f"No {latest_path} found."
        raise FileNotFoundError(msg)
    latest = json.loads(latest_path.read_text())
    run_dir = root / str(latest.get("path", latest["run_id"]))
    meta = RunMeta.from_json(run_dir / "meta.json")
    return run_dir, meta


def plot_all(source: str = "saved") -> None:
    """Generate all available charts for the latest run from a source."""
    run_dir, meta = load_run(source)
    print(f"Plotting benchmark: {meta.commit_short} ({meta.date}) [profile={meta.profile}]")

    PLOTS_DIR.mkdir(exist_ok=True)
    suffix = "" if source == "saved" else f"-{source}"

    if (run_dir / "upload.csv").exists():
        _plot_upload(run_dir, meta, suffix)

    if (run_dir / "list.csv").exists():
        _plot_list(run_dir, meta, suffix)

    if (run_dir / "download.csv").exists():
        _plot_download(run_dir, meta, suffix)

    print("Done.")


def _plot_upload(run_dir: Path, meta: RunMeta, suffix: str) -> None:
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

    fig, (ax_time, ax_rss) = plt.subplots(2, 1, figsize=(12, 9), height_ratios=[3, 2])
    fig.patch.set_facecolor(BG)
    ax_time.set_facecolor(BG)
    ax_rss.set_facecolor(BG)

    _draw_grouped_bars(
        ax_time,
        rows,
        backends,
        tools,
        value_attr="mean",
        err_lo_attr="ci95_half",
        err_hi_attr="ci95_half",
        annotate=lambda r: f"{r.mean:.2f}s",
    )
    ax_time.set_ylabel("Time (seconds, mean ± 95% CI)", fontsize=11, fontfamily="monospace")
    ax_time.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.1f"))
    ax_time.set_xticklabels([])
    ax_time.grid(axis="y", alpha=0.3, zorder=0)
    ax_time.legend(
        loc="upper right",
        frameon=True,
        fancybox=False,
        edgecolor="#1a1614",
        fontsize=10,
        prop={"family": "monospace"},
    )

    _draw_grouped_bars(
        ax_rss,
        rows,
        backends,
        tools,
        value_attr="rss_mb",
        err_lo_attr=None,
        err_hi_attr=None,
        annotate=lambda r: f"{r.rss_mb:.0f}",
    )
    ax_rss.set_ylabel("Peak RSS (MB)", fontsize=11, fontfamily="monospace")
    ax_rss.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.0f"))
    ax_rss.grid(axis="y", alpha=0.3, zorder=0)

    for ax in (ax_time, ax_rss):
        ax.spines["top"].set_visible(False)
        ax.spines["right"].set_visible(False)
        ax.spines["left"].set_color("#1a1614")
        ax.spines["bottom"].set_color("#1a1614")
        ax.tick_params(colors="#1a1614")

    fig.suptitle(
        f"s3z upload  //  {total_mb} MB  //  {meta.commit_short}  //  profile={meta.profile}",
        fontsize=13,
        fontfamily="monospace",
        fontweight="bold",
    )

    machine = f"{meta.os} {meta.arch} / {meta.cpus} cores / {meta.memory_gb} GB"
    fig.text(
        0.5,
        0.01,
        machine,
        ha="center",
        fontsize=9,
        fontfamily="monospace",
        color="#1a1614",
        alpha=0.55,
    )

    plt.tight_layout(rect=(0, 0.04, 1, 0.97))
    out = PLOTS_DIR / f"upload{suffix}.svg"
    fig.savefig(out, bbox_inches="tight", facecolor=BG)
    plt.close(fig)
    print(f"  saved: {out}")


def _plot_list(run_dir: Path, meta: RunMeta, suffix: str) -> None:
    rows = _read_csv(run_dir / "list.csv")
    if not rows:
        print("  skip: no valid list data")
        return

    backends = list(dict.fromkeys(r.backend for r in rows))
    csv_tools = list(dict.fromkeys(r.tool for r in rows))
    tools = [t for t in TOOL_ORDER if t in csv_tools] + [
        t for t in csv_tools if t not in TOOL_ORDER
    ]
    file_count = rows[0].total_mb  # reused field holds file count for list

    fig, (ax_time, ax_rss) = plt.subplots(2, 1, figsize=(12, 9), height_ratios=[3, 2])
    fig.patch.set_facecolor(BG)
    ax_time.set_facecolor(BG)
    ax_rss.set_facecolor(BG)

    _draw_grouped_bars(
        ax_time,
        rows,
        backends,
        tools,
        value_attr="mean",
        err_lo_attr="ci95_half",
        err_hi_attr="ci95_half",
        annotate=lambda r: f"{r.mean:.3f}s",
    )
    ax_time.set_ylabel("Time (seconds, mean ± 95% CI)", fontsize=11, fontfamily="monospace")
    ax_time.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.2f"))
    ax_time.set_xticklabels([])
    ax_time.grid(axis="y", alpha=0.3, zorder=0)
    ax_time.legend(
        loc="upper right",
        frameon=True,
        fancybox=False,
        edgecolor="#1a1614",
        fontsize=10,
        prop={"family": "monospace"},
    )

    _draw_grouped_bars(
        ax_rss,
        rows,
        backends,
        tools,
        value_attr="rss_mb",
        err_lo_attr=None,
        err_hi_attr=None,
        annotate=lambda r: f"{r.rss_mb:.0f}",
    )
    ax_rss.set_ylabel("Peak RSS (MB)", fontsize=11, fontfamily="monospace")
    ax_rss.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.0f"))
    ax_rss.grid(axis="y", alpha=0.3, zorder=0)

    for ax in (ax_time, ax_rss):
        ax.spines["top"].set_visible(False)
        ax.spines["right"].set_visible(False)
        ax.spines["left"].set_color("#1a1614")
        ax.spines["bottom"].set_color("#1a1614")
        ax.tick_params(colors="#1a1614")

    fig.suptitle(
        f"s3z list  //  {file_count} files  //  {meta.commit_short}  //  profile={meta.profile}",
        fontsize=13,
        fontfamily="monospace",
        fontweight="bold",
    )

    machine = f"{meta.os} {meta.arch} / {meta.cpus} cores / {meta.memory_gb} GB"
    fig.text(
        0.5,
        0.01,
        machine,
        ha="center",
        fontsize=9,
        fontfamily="monospace",
        color="#1a1614",
        alpha=0.55,
    )

    plt.tight_layout(rect=(0, 0.03, 1, 0.97))
    out = PLOTS_DIR / f"list{suffix}.svg"
    fig.savefig(out, bbox_inches="tight", facecolor=BG)
    plt.close(fig)
    print(f"  saved: {out}")


def _plot_download(run_dir: Path, meta: RunMeta, suffix: str) -> None:
    rows = _read_csv(run_dir / "download.csv")
    if not rows:
        print("  skip: no valid download data")
        return

    backends = list(dict.fromkeys(r.backend for r in rows))
    csv_tools = list(dict.fromkeys(r.tool for r in rows))
    tools = [t for t in TOOL_ORDER if t in csv_tools] + [
        t for t in csv_tools if t not in TOOL_ORDER
    ]
    total_mb = rows[0].total_mb

    fig, (ax_time, ax_rss) = plt.subplots(2, 1, figsize=(12, 9), height_ratios=[3, 2])
    fig.patch.set_facecolor(BG)
    ax_time.set_facecolor(BG)
    ax_rss.set_facecolor(BG)

    _draw_grouped_bars(
        ax_time,
        rows,
        backends,
        tools,
        value_attr="mean",
        err_lo_attr="ci95_half",
        err_hi_attr="ci95_half",
        annotate=lambda r: f"{r.mean:.2f}s",
    )
    ax_time.set_ylabel("Time (seconds, mean ± 95% CI)", fontsize=11, fontfamily="monospace")
    ax_time.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.1f"))
    ax_time.set_xticklabels([])
    ax_time.grid(axis="y", alpha=0.3, zorder=0)
    ax_time.legend(
        loc="upper right",
        frameon=True,
        fancybox=False,
        edgecolor="#1a1614",
        fontsize=10,
        prop={"family": "monospace"},
    )

    _draw_grouped_bars(
        ax_rss,
        rows,
        backends,
        tools,
        value_attr="rss_mb",
        err_lo_attr=None,
        err_hi_attr=None,
        annotate=lambda r: f"{r.rss_mb:.0f}",
    )
    ax_rss.set_ylabel("Peak RSS (MB)", fontsize=11, fontfamily="monospace")
    ax_rss.yaxis.set_major_formatter(ticker.FormatStrFormatter("%.0f"))
    ax_rss.grid(axis="y", alpha=0.3, zorder=0)

    for ax in (ax_time, ax_rss):
        ax.spines["top"].set_visible(False)
        ax.spines["right"].set_visible(False)
        ax.spines["left"].set_color("#1a1614")
        ax.spines["bottom"].set_color("#1a1614")
        ax.tick_params(colors="#1a1614")

    fig.suptitle(
        f"s3z download  //  {total_mb} MB  //  {meta.commit_short}  //  profile={meta.profile}",
        fontsize=13,
        fontfamily="monospace",
        fontweight="bold",
    )

    machine = f"{meta.os} {meta.arch} / {meta.cpus} cores / {meta.memory_gb} GB"
    fig.text(
        0.5,
        0.01,
        machine,
        ha="center",
        fontsize=9,
        fontfamily="monospace",
        color="#1a1614",
        alpha=0.55,
    )

    plt.tight_layout(rect=(0, 0.03, 1, 0.97))
    out = PLOTS_DIR / f"download{suffix}.svg"
    fig.savefig(out, bbox_inches="tight", facecolor=BG)
    plt.close(fig)
    print(f"  saved: {out}")


def _read_csv(path: Path) -> list[PlotRow]:
    rows: list[PlotRow] = []
    with path.open() as f:
        for row in csv.DictReader(f):
            try:
                mean = float(row.get("mean_s", "0") or 0)
            except ValueError:
                continue
            if mean == 0:
                continue
            rows.append(
                PlotRow(
                    backend=row["backend"],
                    tool=row["tool"],
                    n=int(row.get("n") or row.get("runs") or 0),
                    mean=mean,
                    median=float(row.get("median_s") or mean),
                    stdev=float(row.get("stdev_s") or 0),
                    ci95_half=float(row.get("ci95_half_s") or 0),
                    rss_mb=float(row.get("peak_rss_mb") or 0),
                    total_mb=row.get("total_mb", "?") or "?",
                    workers=row.get("workers", "?") or "?",
                    concurrency=row.get("concurrency", "?") or "?",
                )
            )
    return rows


def _draw_grouped_bars(
    ax,  # noqa: ANN001
    rows: list[PlotRow],
    backends: list[str],
    tools: list[str],
    *,
    value_attr: str,
    err_lo_attr: str | None,
    err_hi_attr: str | None,
    annotate,  # noqa: ANN001
) -> None:
    bar_width = 0.22
    group_gap = 0.15
    n_tools = len(tools)

    for ti, tool in enumerate(tools):
        xs: list[float] = []
        vals: list[float] = []
        err_lo: list[float] = []
        err_hi: list[float] = []
        annotations: list[str] = []

        for bi, backend in enumerate(backends):
            matched = [r for r in rows if r.backend == backend and r.tool == tool]
            if not matched:
                continue
            r = matched[0]
            x = bi * (n_tools * bar_width + group_gap) + ti * bar_width
            xs.append(x)
            vals.append(getattr(r, value_attr))
            err_lo.append(getattr(r, err_lo_attr) if err_lo_attr else 0.0)
            err_hi.append(getattr(r, err_hi_attr) if err_hi_attr else 0.0)
            annotations.append(annotate(r))

        ax.bar(
            xs,
            vals,
            bar_width,
            color=TOOL_COLORS.get(tool, FALLBACK_COLORS[ti % len(FALLBACK_COLORS)]),
            label=tool,
            zorder=3,
            edgecolor="none",
        )
        if err_lo_attr or err_hi_attr:
            ax.errorbar(
                xs,
                vals,
                yerr=[err_lo, err_hi],
                fmt="none",
                ecolor="#1a1614",
                elinewidth=1.2,
                capsize=3,
                zorder=4,
            )

        for x, v, hi, txt in zip(xs, vals, err_hi, annotations, strict=True):
            ax.text(
                x,
                v + hi + (max(vals) * 0.02 if vals else 0.05),
                txt,
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
    ax.set_xticklabels(backends, fontfamily="monospace", fontsize=11, fontweight="bold")
