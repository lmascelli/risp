//! Elapsed workloads complementing the exact fuel-accounting report.
//!
//! Versioned keys describe fixed workloads. Parsing, fixture construction,
//! validation, and final result destruction are outside each timed interval.
use super::metrics::{Measurement, repeat_elapsed};
use super::report::{Report, Row};
use super::suite::{Bench, Meter};
use crate::{LispExp, set_remaining};
use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

const EVAL_ITERS: usize = 2_000;
const LIST_LEN: usize = 4_096;
const WALK_BATCH: usize = 8;
const COMPARE_BATCH: usize = 32;

/// Named closures capture their root, so remove its owned bindings explicitly.
/// Without this cleanup, repeated benchmark fixtures leak root/closure cycles.
struct Fixture(Bench);

impl Fixture {
    fn new(setup: &str) -> Self {
        let fixture = Self(Bench::new());
        if !setup.is_empty() {
            set_remaining(u32::MAX);
            fixture.0.run(&fixture.0.parse(setup));
        }
        fixture
    }
}

pub(super) fn timing(report: &mut Report) {
    evaluator(report);
    list_walks(report);
    equality(report);
    printing(report);
}

fn timed_row(
    key: &str,
    label: &str,
    measurement: Measurement,
    units: usize,
    unit_label: &str,
    note: &str,
) -> Row {
    Row::timed(
        key,
        label,
        measurement.ns_per_unit(units as u64),
        measurement.display_ns_per_unit(units as u64, unit_label),
        note,
    )
}

fn evaluator(report: &mut Report) {
    let mut rows = Vec::new();
    for (key, label, setup, source) in [
        (
            "elapsed/v1/eval/inline",
            "inline arithmetic loop",
            "",
            format!("(setq i 0) (while (< i {EVAL_ITERS}) (setq i (+ i 1))) i"),
        ),
        (
            "elapsed/v1/eval/function",
            "user function loop",
            "(defun perf-inc (x) (+ x 1))",
            format!("(setq i 0) (while (< i {EVAL_ITERS}) (setq i (perf-inc i))) i"),
        ),
        (
            "elapsed/v1/eval/macro",
            "macro expansion loop",
            "(defmacro perf-inc-macro (x) (list '+ x 1))",
            format!("(setq i 0) (while (< i {EVAL_ITERS}) (setq i (perf-inc-macro i))) i"),
        ),
        (
            "elapsed/v1/eval/tail",
            "tail-recursive countdown",
            "(defun perf-countdown (n) (if (= n 0) 2000 (perf-countdown (- n 1))))",
            format!("(perf-countdown {EVAL_ITERS})"),
        ),
    ] {
        let fixture = Fixture::new(setup);
        let ast = fixture.0.parse(&source);
        let elapsed = repeat_elapsed(|| {
            set_remaining(u32::MAX);
            let start = Instant::now();
            let result = black_box(fixture.0.run(black_box(&ast)));
            let elapsed = start.elapsed();
            assert_eq!(result, LispExp::number(EVAL_ITERS as f64));
            elapsed
        });
        rows.push(timed_row(
            key,
            label,
            elapsed,
            EVAL_ITERS,
            "ns/iteration",
            "2,000 iterations; prepared AST; metering included",
        ));
    }
    report.section(
        "EVALUATOR ELAPSED WORK",
        "Wall-clock costs of the same fixed work under inline arithmetic, calls, \
         expansion, and tail calls. Includes evaluator allocations and temporary \
         destruction; excludes parsing and final result destruction. These rows \
         complement fuel counts, which cannot see Rust implementation changes.",
        rows,
    );
}

fn list_walks(report: &mut Report) {
    let fixture = Fixture::new("");
    fixture
        .0
        .env
        .set_variable("perf-items".into(), flat_list(LIST_LEN, None));
    let mut rows = Vec::new();
    for (key, label, source) in [
        ("length", "length of prepared list", "(length perf-items)"),
        (
            "member-miss",
            "member missing element",
            "(member -1 perf-items)",
        ),
        ("reverse", "reverse prepared list", "(reverse perf-items)"),
    ] {
        let ast = fixture.0.parse(source);
        let elapsed = repeat_elapsed(|| {
            // Retain all returned values until the interval is over so reverse
            // measures construction separately from destruction of its output.
            let mut results = Vec::with_capacity(WALK_BATCH);
            set_remaining(u32::MAX);
            let start = Instant::now();
            for _ in 0..WALK_BATCH {
                results.push(black_box(fixture.0.run(black_box(&ast))));
            }
            let elapsed = start.elapsed();
            for result in &results {
                match key {
                    "length" => assert_eq!(result, &LispExp::number(LIST_LEN as f64)),
                    "member-miss" => assert!(result.is_nil()),
                    _ => {
                        assert_eq!(result.iter().count(), LIST_LEN);
                        assert_eq!(
                            result.iter().next(),
                            Some(LispExp::number((LIST_LEN - 1) as f64))
                        );
                    }
                }
            }
            elapsed
        });
        rows.push(timed_row(
            &format!("elapsed/v1/list/{key}"),
            label,
            elapsed,
            LIST_LEN * WALK_BATCH,
            "ns/input-element",
            "4,096-element input; 8 calls; returned values dropped after timing",
        ));
    }
    report.section(
        "PREPARED LIST OPERATIONS",
        "Input lists and syntax are prepared once outside timing. Full traversal \
         is measured, including fuel charges and any temporary allocations. \
         Reverse includes allocating its output; output destruction is excluded.",
        rows,
    );
}

fn flat_list(n: usize, changed: Option<usize>) -> LispExp<Meter> {
    LispExp::proper_list(
        (0..n)
            .map(|i| LispExp::number(if Some(i) == changed { -1.0 } else { i as f64 }))
            .collect(),
    )
}

fn nested_list(depth: usize) -> LispExp<Meter> {
    let mut value = LispExp::number(1.0);
    for _ in 0..depth {
        value = LispExp::cons(value, LispExp::nil());
    }
    value
}

fn equality(report: &mut Report) {
    let make_vector = || LispExp::vec((0..LIST_LEN).map(|i| LispExp::number(i as f64)).collect());
    let make_map = || {
        LispExp::map(
            (0..256)
                .map(|i| (format!("key-{i}"), LispExp::number(i as f64)))
                .collect::<HashMap<_, _>>(),
        )
    };
    let mut rows = Vec::new();
    for (key, label, left, right, expected, note) in [
        (
            "list-equal",
            "equal flat lists",
            flat_list(LIST_LEN, None),
            flat_list(LIST_LEN, None),
            true,
            "4,096 elements; distinct allocations",
        ),
        (
            "list-early",
            "lists differ at head",
            flat_list(LIST_LEN, None),
            flat_list(LIST_LEN, Some(0)),
            false,
            "4,096 elements; mismatch at index 0",
        ),
        (
            "list-late",
            "lists differ at last element",
            flat_list(LIST_LEN, None),
            flat_list(LIST_LEN, Some(LIST_LEN - 1)),
            false,
            "4,096 elements; mismatch at index 4,095",
        ),
        (
            "nested",
            "equal nested car chains",
            nested_list(1_024),
            nested_list(1_024),
            true,
            "1,024 cons levels; distinct allocations",
        ),
        (
            "vector",
            "equal vectors",
            make_vector(),
            make_vector(),
            true,
            "4,096 elements; distinct allocations",
        ),
        (
            "map",
            "equal maps",
            make_map(),
            make_map(),
            true,
            "256 entries; independently seeded maps",
        ),
    ] {
        let elapsed = repeat_elapsed(|| {
            let mut matches = 0;
            let start = Instant::now();
            for _ in 0..COMPARE_BATCH {
                matches += usize::from(black_box(black_box(&left) == black_box(&right)));
            }
            let elapsed = start.elapsed();
            assert_eq!(matches, if expected { COMPARE_BATCH } else { 0 });
            elapsed
        });
        rows.push(timed_row(
            &format!("elapsed/v1/equality/{key}"),
            label,
            elapsed,
            COMPARE_BATCH,
            "ns/comparison",
            note,
        ));
    }
    report.section(
        "STRUCTURAL EQUALITY",
        "Rust PartialEq, also used by Lisp equal, on prepared values. Reports a \
         complete comparison rather than cost per nominal element: early exits \
         inspect fewer elements. Values are retained outside the timed interval.",
        rows,
    );
}

fn printing(report: &mut Report) {
    const PRINT_BATCH: usize = 8;
    let mut rows = Vec::new();
    for (key, label, value) in [
        ("flat", "format flat list", flat_list(1_024, None)),
        ("nested", "format nested car chain", nested_list(1_024)),
    ] {
        let reference = format!("{value:?}");
        let elapsed = repeat_elapsed(|| {
            let mut results = Vec::with_capacity(PRINT_BATCH);
            let start = Instant::now();
            for _ in 0..PRINT_BATCH {
                results.push(black_box(format!("{:?}", black_box(&value))));
            }
            let elapsed = start.elapsed();
            assert!(results.iter().all(|text| text == &reference));
            elapsed
        });
        rows.push(timed_row(
            &format!("elapsed/v1/printing/{key}"),
            label,
            elapsed,
            PRINT_BATCH * reference.len(),
            "ns/output-byte",
            "1,024 elements/levels; output allocation included; output drop excluded",
        ));
    }
    report.section(
        "VALUE FORMATTING",
        "LispExp Debug formatting into a newly allocated String. Input values \
         are prepared outside timing, generated output is checked and released \
         afterwards. Byte normalization permits comparison across formatter changes.",
        rows,
    );
}
