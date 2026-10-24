//! Destruction fixtures shared by elapsed-time and allocation measurements.
//! Construction and the final release of shared keep-alives are excluded.
#[cfg(not(feature = "perf-alloc"))]
use super::metrics::repeat_elapsed;
use super::report::{Report, Row};
use crate::LispExp;
#[cfg(not(feature = "perf-alloc"))]
use std::time::Instant;
use std::{collections::HashMap, hint::black_box};

struct Fixture {
    values: Vec<LispExp<()>>,
    keep_alive: Option<LispExp<()>>,
}

impl Fixture {
    fn owned(values: Vec<LispExp<()>>) -> Self {
        Self {
            values,
            keep_alive: None,
        }
    }
}

struct Case {
    key: &'static str,
    label: &'static str,
    base: usize,
    units_per_item: usize,
    unit: &'static str,
    build: fn(usize) -> Fixture,
}

fn list(n: usize) -> LispExp<()> {
    (0..n).fold(LispExp::nil(), |tail, i| {
        LispExp::cons(LispExp::number(i as f64), tail)
    })
}

fn scalar_cons(n: usize) -> Fixture {
    Fixture::owned(
        (0..n)
            .map(|_| LispExp::cons(LispExp::number(1.0), LispExp::number(2.0)))
            .collect(),
    )
}

fn small_lists(n: usize) -> Fixture {
    Fixture::owned((0..n).map(|_| list(4)).collect())
}

fn flat_list(n: usize) -> Fixture {
    Fixture::owned(vec![list(n)])
}

fn flat_vector(n: usize) -> Fixture {
    Fixture::owned(vec![LispExp::vec(
        (0..n).map(|i| LispExp::number(i as f64)).collect(),
    )])
}

fn flat_map(n: usize) -> Fixture {
    Fixture::owned(vec![LispExp::map(
        (0..n)
            .map(|i| (format!("key-{i}"), LispExp::number(i as f64)))
            .collect(),
    )])
}

fn car_chain(n: usize) -> Fixture {
    let value = (0..n).fold(LispExp::nil(), |child, _| {
        LispExp::cons(child, LispExp::nil())
    });
    Fixture::owned(vec![value])
}

// Keep baseline fixtures safe before vector/map Drop becomes iterative.
// This is a performance workload, not an extreme-depth stack-safety test.
const MIXED_DEPTH: usize = 24;

fn mixed_chains(n: usize) -> Fixture {
    Fixture::owned(
        (0..n)
            .map(|_| {
                (0..MIXED_DEPTH).fold(LispExp::number(1.0), |child, depth| match depth % 4 {
                    0 => LispExp::cons(child, LispExp::nil()),
                    1 => LispExp::vec(vec![child]),
                    2 => LispExp::form(vec![child]),
                    _ => LispExp::map(HashMap::from([("child".into(), child)])),
                })
            })
            .collect(),
    )
}

fn branching(n: usize) -> Fixture {
    Fixture::owned(
        (0..n)
            .map(|_| {
                LispExp::vec(vec![
                    list(4),
                    LispExp::map(HashMap::from([("child".into(), list(4))])),
                    LispExp::form(vec![list(4), list(4)]),
                ])
            })
            .collect(),
    )
}

fn shared(n: usize) -> Fixture {
    let keep_alive = list(4_096);
    let values = (0..n)
        .map(|_| LispExp::cons(keep_alive.clone(), LispExp::nil()))
        .collect();
    Fixture {
        values,
        keep_alive: Some(keep_alive),
    }
}

fn cases() -> [Case; 9] {
    [
        Case {
            key: "scalar-cons",
            label: "scalar cons",
            base: 4_096,
            units_per_item: 1,
            unit: "cons",
            build: scalar_cons,
        },
        Case {
            key: "four-cell-list",
            label: "four-cell lists",
            base: 1_024,
            units_per_item: 4,
            unit: "cons",
            build: small_lists,
        },
        Case {
            key: "flat-list",
            label: "flat list",
            base: 8_192,
            units_per_item: 1,
            unit: "cons",
            build: flat_list,
        },
        Case {
            key: "flat-vector",
            label: "flat vector",
            base: 8_192,
            units_per_item: 1,
            unit: "element",
            build: flat_vector,
        },
        Case {
            key: "flat-map",
            label: "flat map",
            base: 1_024,
            units_per_item: 1,
            unit: "entry",
            build: flat_map,
        },
        Case {
            key: "car-chain",
            label: "nested car chain",
            base: 8_192,
            units_per_item: 1,
            unit: "cons",
            build: car_chain,
        },
        Case {
            key: "mixed-depth24",
            label: "mixed chains, depth 24",
            base: 128,
            units_per_item: MIXED_DEPTH,
            unit: "container",
            build: mixed_chains,
        },
        Case {
            key: "branching",
            label: "branching containers",
            base: 256,
            units_per_item: 19,
            unit: "container",
            build: branching,
        },
        Case {
            key: "shared-child",
            label: "shared child retained",
            base: 4_096,
            units_per_item: 1,
            unit: "parent",
            build: shared,
        },
    ]
}

#[cfg(not(feature = "perf-alloc"))]
pub(super) fn timing(report: &mut Report) {
    let mut rows = Vec::new();
    for case in cases() {
        for factor in [1, 2, 4] {
            let n = case.base * factor;
            let units = n * case.units_per_item;
            let measured = repeat_elapsed(|| {
                let Fixture { values, keep_alive } = (case.build)(n);
                let start = Instant::now();
                drop(black_box(values));
                let elapsed = start.elapsed();
                black_box(&keep_alive);
                drop(keep_alive);
                elapsed
            });
            rows.push(Row::timed(
                format!("drop/v1/{}/n{n}/ns", case.key),
                format!("{} ({units})", case.label),
                measured.ns_per_unit(units as u64),
                measured.display_ns_per_unit(units as u64, &format!("ns/{}", case.unit)),
                format!(
                    "drop only; median total {:.3} us",
                    measured.as_secs_f64() * 1e6
                ),
            ));
        }
    }
    report.section(
        "DESTRUCTION",
        "Fresh fixtures are constructed outside every measured interval. Only their release is timed; the outer Rust batch Vec release is included. Rows at N, 2N and 4N show both per-unit cost and scaling. Shared children stay alive throughout the interval. Mixed depth is intentionally bounded so pre-fix implementations can run this comparison.",
        rows,
    );
    threaded_timing(report);
}

#[cfg(not(feature = "perf-alloc"))]
fn threaded_timing(report: &mut Report) {
    use std::sync::{Arc, Barrier, mpsc};
    const PER_THREAD: usize = 16_384;
    const THREADS: usize = 2;
    let elapsed = repeat_elapsed(|| {
        let barrier = Arc::new(Barrier::new(THREADS + 1));
        let (ready, received) = mpsc::channel();
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let barrier = barrier.clone();
                let ready = ready.clone();
                std::thread::spawn(move || {
                    let fixture = scalar_cons(PER_THREAD);
                    ready.send(()).unwrap();
                    barrier.wait();
                    drop(black_box(fixture.values));
                })
            })
            .collect();
        for _ in 0..THREADS {
            received.recv().unwrap();
        }
        let start = Instant::now();
        barrier.wait();
        for handle in handles {
            handle.join().unwrap();
        }
        start.elapsed()
    });
    report.section(
        "CONCURRENT DESTRUCTION",
        "Two threads release independent scalar-cons batches concurrently. Spawn and fixture construction are excluded; the release barrier, joins and thread teardown are included. This is a contention-sensitive end-to-end measure, not an isolated per-thread destructor latency. Allocation counters intentionally do not cover other threads.",
        vec![Row::timed(
            "drop/v1/two-threads-scalar/n16384/ns",
            "two threads, 16,384 cons each",
            elapsed.ns_per_unit((PER_THREAD * THREADS) as u64),
            elapsed.display_ns_per_unit((PER_THREAD * THREADS) as u64, "wall ns/cons"),
            "fixed two-thread workload; scheduling and allocator contention can add noise",
        )],
    );
}

#[cfg(feature = "perf-alloc")]
pub(super) fn allocations(report: &mut Report) {
    let mut rows = Vec::new();
    for case in cases() {
        for factor in [1, 2, 4] {
            let n = case.base * factor;
            let units = n * case.units_per_item;
            // Exclude lazy symbol/container initialization from the window.
            let Fixture { values, keep_alive } = (case.build)(n);
            drop(black_box(values));
            drop(keep_alive);
            let Fixture { values, keep_alive } = (case.build)(n);
            let (_, counts) = super::allocations::measure(|| drop(black_box(values)));
            black_box(&keep_alive);
            drop(keep_alive);
            let prefix = format!("drop-alloc/v1/{}/n{n}", case.key);
            let label = format!("{} ({units})", case.label);
            for (key, suffix, count, note) in [
                (
                    "alloc",
                    "alloc calls",
                    counts.allocs,
                    "successful alloc + alloc_zeroed requests during drop",
                ),
                (
                    "realloc",
                    "realloc calls",
                    counts.reallocs,
                    "successful realloc requests during drop",
                ),
                (
                    "bytes",
                    "requested bytes",
                    counts.requested_bytes,
                    "sum of allocation/reallocation request sizes; not RSS or peak memory",
                ),
            ] {
                rows.push(Row::new(
                    format!("{prefix}/{key}"),
                    format!("{label}: {suffix}"),
                    count as f64,
                    count.to_string(),
                    note,
                ));
            }
        }
    }
    report.section("DESTRUCTION ALLOCATIONS", "Fixture construction and shared-child final release are excluded. Counts cover only the calling thread and Rust's instrumented System allocator. Zero is a valid result. These measurements characterize implementation costs; they do not impose a zero-allocation requirement.", rows);
}
