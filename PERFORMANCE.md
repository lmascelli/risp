# Performance checks

The interpreter suite is excluded from ordinary `cargo test` runs. Run it
alone so unit tests do not compete for CPU time:

```sh
PERF_MACHINE=my-workstation cargo test tests::perf::interpreter_performance_suite -- --ignored --exact --test-threads=1
```

Each elapsed-time row reports the median and min–max of nine measured samples.
Timing ratios and targets are advisory. Fuel scaling and list-primitive fuel
charges are exact accounting checks; they do not measure Rust execution time.

Reports (`performance-cost.txt` and `performance-timing.txt`) are generated and
ignored by Git. Baselines are separate versioned files. After reviewing a run
on the intended reference machine, save its baseline explicitly:

```sh
PERF_MACHINE=my-workstation PERF_BLESS=1 cargo test tests::perf::interpreter_performance_suite -- --ignored --exact --test-threads=1
```

Review and commit `performance-cost.baseline` and
`performance-timing.baseline`. Future runs with the same `PERF_MACHINE` label,
build profile, target and CPU count show deltas against those files. Baselines
are left untouched unless `PERF_BLESS=1` is set.

## JSON

The JSON reader and writer have their own timing run on a generated document
of just over 1 MiB, shaped like a batch of editor diagnostics. Run it in a
release build:

```sh
cargo test --release json_timing -- --ignored --nocapture
```

It prints the median of nine samples for parsing, serializing and dropping
the parsed value, and checks that the written text parses back equal.
