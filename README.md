<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="public/banner-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="public/banner-cream.svg">
    <img alt="s3z — S3 ops, but fearlessly fast" src="public/banner-cream.svg" width="800">
  </picture>
</p>

<p align="center">
  <strong>S3 ops, but fearlessly fast.</strong><br>
  <sub>Built in Rust. Streaming. Parallel. No bloat.</sub>
</p>

A lightweight, high-throughput S3 client built on raw HTTP and SigV4 signing.

## Development

### Benchmarks

Two profiles. **`dev`** is the inner loop (one backend, three tools),
run it before and after each change. **`full`** is the canonical run
(four backends, four tools) — run it at milestones, commit the result.

Every run reports `mean ± 95% CI` (not min/max — those grow with `n` and mislead)
plus peak RSS per cell, so wall-time and memory regressions surface together.
Sample count is adaptive: keep sampling until CI half-width drops below 5% of
the mean, or `max_runs` is hit.

**Inner-loop workflow.** Baseline before, measure after, compare statistically:

```sh
mise run bench:dev          # baseline
mise run bench:save         # → target/bench/baseline/  (gitignored)

# ... edit code ...

mise run bench:dev          # measure
mise run bench:plot         # visual sanity check
mise run bench:compare      # exits non-zero on regression
```

`compare` flags a cell only when **both** Welch's t-test (p<0.05) **and**
the absolute change exceeds the noise floor. False alarms from jittery cells
are suppressed. Run `mise run bench:noise` once per machine to write
`benchmarks/noise.json` — that sets the threshold; without it the default
is ±5% / ±0.05s.

**Full run.** When you want the committed reference numbers:

```sh
mise run bench           # full benchmark
mise run bench:save      # → benchmarks/<run-id>/  (committed)
git add benchmarks/
```

**Sweeps** are opt-in (slow, only useful when investigating an axis):

```sh
mise run bench:sweep size            # file size: 8 → 32 → 128 → 512 MB
mise run bench:sweep concurrency     # workers × concurrency: 4×1 → 64×8
```

**Adding a new operation** is one file in `bench/operations/`. The harness
handles backend lifecycle, sampling, RSS/CPU collection, and CSV output —
see `bench/operations/upload.py` for the template and `bench/operations/_api.py`
for the contract.

## License

MIT
