# Comparing interpreter implementations

These ignored suites characterize charged Lisp work, elapsed time and allocator
requests separately. They are not part of ordinary `cargo test`. No dependency
on a benchmark framework or external profiler is required.

## What is measured

| Report | Measurements | Meaning |
| --- | --- | --- |
| `performance-cost.txt` | Evaluator, calls, list primitives at N/2N/4N | Exact fuel accounting; not CPU time or actual algorithmic complexity |
| `performance-timing.txt` | Parse only, AST release, combined parse/release; evaluator loops/calls/macros/tail calls; scope lookup; prepared list operations; equality; formatting; destruction | Real elapsed time, with explicitly documented fixture boundaries |
| `performance-allocations.txt` | Destruction, list construction, warmed parsing, comparison, formatting, user-function call | Successful Rust allocator requests on the calling thread, measured separately from time |

Destruction cases cover scalar-only cons, four-cell lists, long flat lists,
vectors/maps, nested `car` chains, mixed depth-24 chains, branching containers
and shared children. N/2N/4N rows expose scaling as well as constant costs.
Normalization counts cons cells, elements, entries or containers as indicated;
different shapes are not interchangeable units. A two-thread elapsed workload
also exercises independent scalar-cons destruction under contention.

Tiny destruction operations are batched. Every sample gets fresh fixtures built
outside the interval. Destruction includes releasing the outer Rust batch Vec;
shared-child final release is excluded. Allocation windows get an uncounted
warm-up before constructing the measured fixture. Mixed nesting is deliberately
modest so implementations that still recursively release vectors/maps can run.
Extreme-depth stack-safety checks belong in separate functional tests.

Parsing uses prepared source text. The parse-only interval retains its AST until
the timer stops; the drop-only interval receives a freshly parsed AST. The combined
interval includes both. Different cache histories and noise mean the medians
need not add up. Symbols are warmed by an excluded first invocation; these are
steady-state measurements, not cold process-start measurements.

Equality inputs use distinct allocations except where sharing is the workload.
Early and late mismatches are separate. Formatting includes output allocation,
but excludes output validation and destruction. List reverse includes output
construction; its output is retained until after timing.

## Apply the same suite to A and B

Use the same compiler, target, features and optimization settings, on the same
machine with little background activity. Run timing workloads sequentially.
Use release mode for conclusions; debug runs are useful for functional checks.

The report fingerprint includes a machine label, platform, available CPU count,
debug-assertion setting, a runtime-observed `rustc --version` and an optional build
label. It cannot prove which compiler built an executable, identify CPU frequency
or detect every flag change. Keep the real build settings consistent.

From the repository root, first save A as the baseline:

```sh
PERF_MACHINE=local-dev PERF_BUILD_ID=release-default \
PERF_OUTPUT_DIR=perf-results/A PERF_BLESS=1 \
cargo test --release tests::perf::interpreter_performance_suite \
  -- --ignored --exact --test-threads=1

PERF_MACHINE=local-dev PERF_BUILD_ID=release-default \
PERF_OUTPUT_DIR=perf-results/A PERF_BLESS=1 \
cargo test --release --features perf-alloc \
  tests::perf::interpreter_allocation_suite \
  -- --ignored --exact --test-threads=1
```

Review the reports, then apply implementation B and run without blessing:

```sh
PERF_MACHINE=local-dev PERF_BUILD_ID=release-default \
PERF_OUTPUT_DIR=perf-results/B \
cargo test --release tests::perf::interpreter_performance_suite \
  -- --ignored --exact --test-threads=1

PERF_MACHINE=local-dev PERF_BUILD_ID=release-default \
PERF_OUTPUT_DIR=perf-results/B \
cargo test --release --features perf-alloc \
  tests::perf::interpreter_allocation_suite \
  -- --ignored --exact --test-threads=1
```

`PERF_OUTPUT_DIR` separates report files; baselines always remain at the
repository root as `performance-{cost,timing,allocations}.baseline`. Preserve
these files when switching implementations. With separate checkouts, copy the
baseline files from A's root to B's root. Without matching machine/build labels,
elapsed/allocation baseline deltas are suppressed. New rows are marked `new`.

Do not leave `PERF_BLESS` exported while comparing B: it overwrites the reference.
Repeat runs, ideally alternating A/B, and retain each run in a distinct output
directory. A negative delta means less time, fewer requests or fewer requested
bytes; fuel changes should be interpreted as accounting changes, not speedups.

## Reading results

- Time summaries contain nine samples after one warm-up: median and observed
  min/max. These are not confidence intervals. The fixed 10% annotation is a
  heuristic, not a statistical significance test.
- Wall-clock regressions do not automatically fail the suite. Functional and
  fuel-accounting checks do. Unchanged fuel does not establish unchanged speed.
- Absolute ns/unit values matter even when both implementations scale linearly.
  Early-exit equality reports ns/comparison rather than pretending every input
  element was inspected.
- Allocation counts are collected in a separate binary with `perf-alloc`.
  Allocator hooks add overhead even outside counting windows, so the timing test
  is excluded from that build. The feature changes no production allocator:
  its global allocator is compiled only into this crate's unit-test binary.
- `alloc calls` counts successful `alloc` and `alloc_zeroed`; `realloc calls`
  counts successful resize requests. `requested bytes` sums full request sizes,
  including each new realloc size. It is not net growth, peak live memory or RSS.
  Setup and returned-value destruction are outside operation windows. Drop-only
  windows naturally include temporary allocations and frees inside destructors.
- Counters cover this thread only and exclude libraries bypassing Rust's global
  allocator. Compiler-eliminated allocations are not observable. No zero-allocation
  invariant is imposed on the interpreter.
- `HashMap` uses randomized hashing; iteration order and allocation details can
  vary. Treat map timing differences cautiously and repeat comparisons.
- The concurrent row includes its release barrier, joins and thread teardown,
  but excludes spawn and fixture setup. It measures wall time, not summed CPU
  time, and is especially sensitive to scheduling and available cores.

## Extending the suite

Put fixtures outside timing/counting windows, consume results through
`std::hint::black_box`, validate afterwards and release outputs explicitly after
stopping the timer. Reset fuel before repeated evaluator workloads. Benchmark
environments clear their owned bindings on destruction to break fixture-created
root/closure cycles; this does not fix interpreter-wide cycle collection.

Give new workload definitions stable versioned keys. If size, normalization,
fixture shape, timing boundaries or operation semantics change, bump the key's
version rather than comparing unlike measurements against an old baseline.
Keep unchanged definitions when comparing two implementations.

Useful follow-ups include an allocation profiler for peak live memory and
end-to-end application workloads. Those answer different questions from these
isolated interpreter measurements.
