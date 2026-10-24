//! Allocation windows complementing the destruction-only measurements.
use super::allocations::{Counts, measure};
use super::report::{Report, Row};
use super::suite::Bench;
use crate::{LispExp, Parser, set_remaining};
use std::hint::black_box;

fn rows(key: &str, label: &str, counts: Counts) -> Vec<Row> {
    [
        ("alloc", "alloc calls", counts.allocs),
        ("realloc", "realloc calls", counts.reallocs),
        ("bytes", "requested bytes", counts.requested_bytes),
    ]
    .into_iter()
    .map(|(suffix, metric, value)| {
        Row::new(
            format!("allocation/v1/{key}/{suffix}"),
            format!("{label}: {metric}"),
            value as f64,
            value.to_string(),
            "one operation; inputs prepared outside counting; output release excluded",
        )
    })
    .collect()
}

pub(super) fn run(report: &mut Report) {
    let mut measured = Vec::new();
    let make_list =
        || LispExp::<()>::proper_list((0..4_096).map(|i| LispExp::number(i as f64)).collect());
    let (list, counts) = measure(|| black_box(make_list()));
    measured.extend(rows("build-list4096", "build 4,096-cell list", counts));
    let other = make_list();

    // Warm symbol interning and any one-time implementation state.
    black_box(&list == &other);
    let (equal, counts) = measure(|| black_box(black_box(&list) == black_box(&other)));
    assert!(equal);
    measured.extend(rows(
        "equal-list4096",
        "compare equal 4,096-cell lists",
        counts,
    ));

    drop(format!("{list:?}"));
    let (text, counts) = measure(|| black_box(format!("{:?}", black_box(&list))));
    assert!(text.starts_with("(0 1 2 "));
    measured.extend(rows("format-list4096", "format 4,096-cell list", counts));
    drop(text);

    let source = format!("(progn {})", "(+ 1 2) ".repeat(512));
    drop(
        Parser::new(&source)
            .next::<()>()
            .expect("fixture must parse"),
    );
    let (ast, counts) = measure(|| black_box(Parser::new(black_box(&source)).next::<()>()));
    let ast = ast.expect("fixture must parse");
    measured.extend(rows(
        "parse512-warm",
        "parse 512 forms, warmed symbols",
        counts,
    ));
    drop(ast);

    let bench = Bench::new();
    bench.run(&bench.parse("(defun perf-alloc-inc (x) (+ x 1))"));
    let ast = bench.parse("(perf-alloc-inc 41)");
    set_remaining(u32::MAX);
    drop(bench.run(&ast));
    set_remaining(u32::MAX);
    let (value, counts) = measure(|| black_box(bench.run(black_box(&ast))));
    assert_eq!(value, LispExp::number(42.0));
    measured.extend(rows(
        "function-call",
        "one prepared user-function call",
        counts,
    ));

    report.section(
        "OPERATION ALLOCATIONS",
        "Prepared, warmed operations help track future changes in parsing, value construction, comparison, formatting and call frames. List construction intentionally includes its input Vec and cons allocations. Counters include successful Rust System allocator requests on this thread only. Outputs are retained until the window closes; bytes are cumulative requested sizes, not live or peak memory. Allocations optimized away by the compiler cannot be counted.",
        measured,
    );
}
