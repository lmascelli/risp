//! Standalone performance characterization for the interpreter.
//!
//! Run only with:
//! `cargo test tests::perf::interpreter_performance_suite -- --ignored --exact --test-threads=1`
mod interpreter;
mod metrics;
mod report;

use report::{Kind, Report};
#[test]
#[ignore = "run explicitly with the documented performance command"]
fn interpreter_performance_suite() {
    let mut cost = Report::new(
        Kind::Cost,
        "performance-cost.txt",
        "risp performance: interpreter fuel accounting",
        "Fuel units describe reductions and the per-element charges assigned to list primitives. They are exact and machine-independent; they do not measure Rust execution time.".into(),
    );
    interpreter::cost(&mut cost);
    let mut failures = cost.finish();

    let mut timing = Report::new(
        Kind::Timing,
        "performance-timing.txt",
        "risp performance: elapsed time",
        "Each duration is the median of nine measured repetitions, with the observed min–max range shown. Ratios use medians. Timing targets are advisory; only fuel-accounting and functional checks fail the command.".into(),
    );
    interpreter::timing(&mut timing);
    failures.extend(timing.finish());

    assert!(
        failures.is_empty(),
        "{} behavioral or fuel-accounting check(s) failed; see performance-cost.txt and performance-timing.txt:\n  - {}",
        failures.len(),
        failures.join("\n  - ")
    );
}

mod suite {
    use crate::{Env, EvalError, FuelMeter, LispContext, LispExp, Parser, eval, measure};

    /// Minimal metering host so interpreter costs do not include editor work.
    #[derive(Debug, Clone)]
    pub(super) struct Meter {
        fuel: Arc<FuelMeter>,
    }

    use std::sync::Arc;

    impl Meter {
        fn new() -> Self {
            Self {
                fuel: Arc::new(FuelMeter::new(u32::MAX)),
            }
        }
    }

    impl PartialEq for Meter {
        fn eq(&self, other: &Self) -> bool {
            Arc::ptr_eq(&self.fuel, &other.fuel)
        }
    }

    impl LispContext for Meter {
        fn consume_fuel(&self, amount: u32) -> Result<(), EvalError<Meter>> {
            self.fuel.consume(amount).map_err(|_| EvalError::OutOfFuel)
        }

        fn log_diagnostic(&self, _msg: &str) {}
    }

    pub(super) struct Bench {
        pub ctx: Meter,
        pub env: Arc<Env<Meter>>,
    }

    impl Bench {
        pub fn new() -> Self {
            let env = Env::new_root();
            crate::setup_base_env(env.clone());
            Self {
                ctx: Meter::new(),
                env,
            }
        }

        pub fn parse(&self, src: &str) -> LispExp<Meter> {
            Parser::new(&format!("(progn {src})"))
                .next()
                .unwrap_or_else(|e| panic!("benchmark source failed to parse: {e:?}\n{src}"))
        }

        pub fn run(&self, ast: &LispExp<Meter>) -> LispExp<Meter> {
            eval(ast, self.env.clone(), &self.ctx).expect("benchmark script failed to evaluate")
        }

        pub fn cost(&self, ast: &LispExp<Meter>) -> u64 {
            measure(&self.ctx.fuel, || {
                self.run(ast);
            })
            .1
        }
    }

    /// Exact fuel scaling at `n`, `2n` and `4n`.
    pub(super) struct Scaling {
        pub n: usize,
        pub c1: u64,
        pub c2: u64,
        pub c4: u64,
    }

    impl Scaling {
        pub fn measure(bench: &Bench, n: usize, build: impl Fn(usize) -> String) -> Self {
            let cost_at = |size: usize| bench.cost(&bench.parse(&build(size)));
            Self {
                n,
                c1: cost_at(n),
                c2: cost_at(n * 2),
                c4: cost_at(n * 4),
            }
        }

        pub fn per_unit(&self) -> f64 {
            (self.c2 as f64 - self.c1 as f64) / self.n as f64
        }

        pub fn shape(&self) -> f64 {
            let first = self.c2 as f64 - self.c1 as f64;
            (self.c4 as f64 - self.c2 as f64) / first.max(f64::EPSILON)
        }

        pub fn counts(&self) -> String {
            format!(
                "{} at {}, {} at {}, {} at {}",
                self.c1,
                self.n,
                self.c2,
                self.n * 2,
                self.c4,
                self.n * 4
            )
        }
    }

    pub(super) const SHAPE_TOLERANCE: f64 = 0.02;
}
