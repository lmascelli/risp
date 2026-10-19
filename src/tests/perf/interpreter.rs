//! Interpreter benchmarks using the minimal metering host in [`super::suite`].
use super::metrics::{per_unit_ns, ratio, time_median};
use super::report::{Report, Row};
use super::suite::{Bench, Meter, SHAPE_TOLERANCE, Scaling};
use crate::{LispExp, Parser};

/// Iterations per scaling measurement. Large enough that the per-unit cost
/// dominates the fixed setup, small enough that four sizes stay quick.
const N: usize = 2_000;

/// Add the exact-cost sections. Every assertion in the suite is made here.
pub(super) fn cost(report: &mut Report) {
    eval_core(report);
    calls(report);
    list_primitives(report);
}

fn eval_core(report: &mut Report) {
    let bench = Bench::new();
    bench.run(&bench.parse("(defun countdown (n) (if (= n 0) 0 (countdown (- n 1))))"));

    let loop_ = Scaling::measure(&bench, N, |n| {
        format!("(setq i 0) (while (< i {n}) (setq i (+ i 1)))")
    });
    let tail = Scaling::measure(&bench, N, |n| format!("(countdown {n})"));

    // This depth also checks trampoline stack safety without a second workload.
    bench.run(&bench.parse("(countdown 100000)"));

    report.section(
        "EVALUATOR CORE LOOP",
        "Special-form dispatch, variable lookup and primitive invocation, exercised\n\
         together. The broadest health check there is: anything that made per-step\n\
         cost depend on how much work had already been done -- a growing environment\n\
         chain, an unbounded frame stack, a cache that never evicts -- shows up here\n\
         before it shows up anywhere else.",
        vec![
            scaling_row(
                "eval/loop-iteration",
                "while iteration",
                &loop_,
                "units/iteration",
            ),
            scaling_row("eval/tail-call", "tail call", &tail, "units/frame"),
        ],
    );

    linear(report, "while iteration", &loop_);
    linear(report, "tail call", &tail);
    report.verdict(
        true,
        "tail-call stack",
        "100,000 tail-recursive frames ran without growing the Rust stack",
    );
}

fn calls(report: &mut Report) {
    let bench = Bench::new();
    bench.run(&bench.parse("(defun inc (x) (+ x 1))"));
    bench.run(&bench.parse("(defmacro inc-macro (x) (list '+ x 1))"));

    let inline = Scaling::measure(&bench, N, |n| {
        format!("(setq i 0) (while (< i {n}) (setq i (+ i 1)))")
    });
    let called = Scaling::measure(&bench, N, |n| {
        format!("(setq i 0) (while (< i {n}) (setq i (inc i)))")
    });
    let expanded = Scaling::measure(&bench, N, |n| {
        format!("(setq i 0) (while (< i {n}) (setq i (inc-macro i)))")
    });

    let call_overhead = called.per_unit() - inline.per_unit();
    let macro_overhead = expanded.per_unit() - called.per_unit();

    report.section(
        "CALLS",
        "What a user-defined call costs over the same arithmetic written inline:\n\
         argument evaluation, a child `Env`, parameter binding, the trampolined body.\n\
         And what a macro costs over a function -- macros are re-expanded on every\n\
         evaluation, never cached, so each invocation rebuilds its expansion before it\n\
         can run it. These are the numbers that would move if frames were pooled, if\n\
         binding stopped allocating per call, or if expansion were memoised.",
        vec![
            scaling_row("call/inline", "inline (+ i 1)", &inline, "units/iteration"),
            scaling_row("call/function", "via (inc i)", &called, "units/iteration"),
            scaling_row(
                "call/macro",
                "via (inc-macro i)",
                &expanded,
                "units/iteration",
            ),
            Row::new(
                "call/function-overhead",
                "  function overhead",
                call_overhead,
                format!("{call_overhead:+.3}"),
                "units a call adds over inline",
            ),
            Row::new(
                "call/macro-overhead",
                "  macro overhead",
                macro_overhead,
                format!("{macro_overhead:+.3}"),
                "units expansion adds over a call",
            ),
        ],
    );

    linear(report, "function call", &called);
    linear(report, "macro call", &expanded);
}

fn list_primitives(report: &mut Report) {
    let bench = Bench::new();

    // `cons` must share its tail so cons-then-reverse accumulation remains linear.
    let cons = Scaling::measure(&bench, N, |n| {
        format!(
            "(setq acc nil) (setq i 0) \
             (while (< i {n}) (setq acc (cons i acc)) (setq i (+ i 1))) \
             (length acc)"
        )
    });

    // Walking primitives are measured on a list built *outside* the
    // measurement, so what is counted is the walk and not the construction.
    let walk = |src: &'static str| -> Scaling {
        let bench = Bench::new();
        Scaling::measure(&bench, N, move |n| {
            format!(
                "(setq lst nil) (setq i 0) \
                 (while (< i {n}) (setq lst (cons i lst)) (setq i (+ i 1)))"
            ) + " "
                + src
        })
    };
    // Each of these includes the same construction loop, so the *shape* is what
    // matters here; the per-element figure is construction plus the walk.
    let length = walk("(length lst)");
    let reverse = walk("(reverse lst)");
    let member = walk("(member -1 lst)");

    // The cost the walk itself adds, isolated by differencing against a run
    // that builds the identical list and does nothing with it.
    let build_only = walk("nil");
    let per_element = |s: &Scaling| s.per_unit() - build_only.per_unit();

    report.section(
        "LIST PRIMITIVES",
        "Accumulation, and the primitives that walk what was accumulated.\n\
         The per-element charge on the walking primitives is not bookkeeping: the\n\
         evaluator charges per reduction step, so before these were priced by length\n\
         `(length lst)` cost one unit whether the list held three elements or a\n\
         hundred thousand, and the execution budget bounded the number of steps\n\
         rather than the amount of work. The execution-budget section of the timing\n\
         report shows what that cost in practice.",
        vec![
            scaling_row("list/cons", "cons accumulation", &cons, "units/element"),
            scaling_row(
                "list/build",
                "build only (control)",
                &build_only,
                "units/element",
            ),
            scaling_row(
                "list/length",
                "build + (length lst)",
                &length,
                "units/element",
            ),
            scaling_row(
                "list/reverse",
                "build + (reverse lst)",
                &reverse,
                "units/element",
            ),
            scaling_row(
                "list/member",
                "build + (member .. lst)",
                &member,
                "units/element",
            ),
            Row::new(
                "list/length-walk",
                "  (length lst) alone",
                per_element(&length),
                format!("{:.3}", per_element(&length)),
                "units per element walked",
            ),
            Row::new(
                "list/reverse-walk",
                "  (reverse lst) alone",
                per_element(&reverse),
                format!("{:.3}", per_element(&reverse)),
                "units per element walked",
            ),
            Row::new(
                "list/member-walk",
                "  (member .. lst) alone",
                per_element(&member),
                format!("{:.3}", per_element(&member)),
                "units per element walked",
            ),
        ],
    );

    linear(report, "cons accumulation", &cons);
    linear(report, "(length LIST)", &length);
    linear(report, "(reverse LIST)", &reverse);
    linear(report, "(member ELT LIST)", &member);

    for (name, cost) in [
        ("length", per_element(&length)),
        ("reverse", per_element(&reverse)),
        ("member", per_element(&member)),
    ] {
        report.verdict(
            cost >= 0.9,
            format!("{name} charges per element"),
            format!(
                "`{name}` charges {cost:.2} units per element walked -- a list-walking \
                 primitive must be priced by length, or the budget bounds steps, not work"
            ),
        );
    }
}

pub(super) fn timing(report: &mut Report) {
    parser(report);
    scope_depth(report);
}

fn parser(report: &mut Report) {
    // Parsing is not fuel-metered; measure its scaling with elapsed time.
    let build = |n: usize| {
        let mut src = String::from("(progn ");
        for i in 0..n {
            src.push_str(&format!(
                "(defun helper-{i} (a b) \"Doc.\" (if (< a b) (+ a b) (list a b {i}))) "
            ));
        }
        src.push(')');
        src
    };

    // Use a workload long enough to reduce scheduler noise in timing ratios.
    const FORMS: usize = 4_000;
    let time_parse = |src: &str| {
        time_median(|| {
            let ast: LispExp<Meter> = Parser::new(src).next().expect("source must parse");
            assert!(matches!(ast, LispExp::Form(_)));
        })
    };
    let small = time_parse(&build(FORMS));
    let large = time_parse(&build(FORMS * 2));

    let ns = per_unit_ns(small, FORMS as u64);
    let growth = ratio(large.as_secs_f64(), small.as_secs_f64());

    report.section(
        "PARSER",
        "Source text to AST. Not fuel-charged -- nothing is evaluating yet -- so this\n\
         is one of the few interpreter costs that only a clock can see.",
        vec![
            Row::timed(
                "parser/form-ns",
                "parse one form",
                ns,
                small.display_ns_per_unit(FORMS as u64, "ns/form"),
                "median; fixed 4,000-form source",
            ),
            Row::new(
                "parser/growth",
                "doubling the source",
                growth,
                format!("{growth:.2}x"),
                "advisory target: below 3x",
            ),
        ],
    );
}

fn scope_depth(report: &mut Report) {
    // Lookup cost grows with lexical depth, but fuel charges it as one step.
    // Measure elapsed time to expose the additional parent-chain traversal.
    const ITERS: usize = 20_000;
    const DEPTH: usize = 12;

    let bench = Bench::new();
    // `i` is created at the root first, so the `setq` inside the nested `let`s
    // updates that root binding -- walking the whole chain to reach it --
    // rather than shadowing it locally.
    bench.run(&bench.parse("(setq i 0)"));

    let nested = |depth: usize| {
        let mut src = String::new();
        for d in 0..depth {
            src.push_str(&format!("(let ((pad{d} 0)) "));
        }
        src.push_str(&format!(
            "(setq i 0) (while (< i {ITERS}) (setq i (+ i 1)))"
        ));
        src.push_str(&")".repeat(depth));
        bench.parse(&src)
    };

    let shallow_ast = nested(0);
    let deep_ast = nested(DEPTH);
    let shallow = time_median(|| {
        bench.run(&shallow_ast);
    });
    let deep = time_median(|| {
        bench.run(&deep_ast);
    });

    let ns = per_unit_ns(shallow, ITERS as u64);
    let penalty = ratio(deep.as_secs_f64(), shallow.as_secs_f64());

    report.section(
        "SCOPE CHAIN",
        "The same loop run at the top level and inside 12 nested `let`s. Lookup walks\n\
         the environment chain by name, so the gap between these two is what nesting\n\
         costs. Lexical addressing would collapse it toward 1.00x.",
        vec![
            Row::timed(
                "eval/step-ns",
                "eval step at depth 0",
                ns,
                shallow.display_ns_per_unit(ITERS as u64, "ns/iteration"),
                "median; loop at scope depth 0",
            ),
            Row::new(
                "scope/depth-penalty",
                format!("depth {DEPTH} vs depth 0"),
                penalty,
                format!("{penalty:.2}x"),
                "advisory target: below 12x",
            ),
        ],
    );
}

fn scaling_row(key: &str, label: &str, s: &Scaling, unit: &str) -> Row {
    Row::new(
        key,
        label,
        s.per_unit(),
        format!("{:.3}", s.per_unit()),
        format!("{unit}; shape {:.3}  [{}]", s.shape(), s.counts()),
    )
}

/// Assert that an operation is exactly linear, and record the check.
fn linear(report: &mut Report, name: &str, s: &Scaling) {
    let shape = s.shape();
    report.verdict(
        (shape - 2.0).abs() <= SHAPE_TOLERANCE,
        format!("{name} is linear"),
        format!(
            "{name} scales at {shape:.3} (2.000 is linear, 4.000 quadratic) -- {}",
            s.counts()
        ),
    );
}
