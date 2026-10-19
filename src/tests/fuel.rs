#[cfg(test)]
mod tests {
    use crate::fuel::Exhausted;
    use crate::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, RwLock};

    #[test]
    fn a_nested_scope_does_not_refill_the_budget() {
        // Reentrant evaluation must spend the enclosing scope's remaining budget.
        let meter = FuelMeter::new(1_000);

        let outer = meter.begin();
        meter.consume(400).expect("600 should remain");
        {
            let _inner = meter.begin();
            meter.consume(400).expect("200 should remain");
        }
        assert!(
            meter.consume(300).is_err(),
            "the inner scope refilled the budget -- nested evaluation can launder its own limit"
        );
        drop(outer);
    }

    #[test]
    fn a_fresh_outermost_scope_does_refill() {
        let meter = FuelMeter::new(1_000);
        {
            let _scope = meter.begin();
            meter.consume(900).expect("first scope has a full budget");
        }
        let _scope = meter.begin();
        meter
            .consume(900)
            .expect("a new outermost scope must start from a full budget");
    }

    #[test]
    fn consume_spends_the_last_unit_and_then_reports_exhaustion() {
        // Spending the final unit succeeds; the following charge must not wrap the counter.
        let meter = FuelMeter::new(10);
        let _scope = meter.begin();
        meter
            .consume(10)
            .expect("spending exactly the remaining budget must succeed");
        assert_eq!(meter.consume(1).unwrap_err(), Exhausted);
    }

    // Spawned threads must use the configured budget and keep fuel independent
    // of their parent, both through the meter and the Lisp `spawn` form.

    #[test]
    fn a_spawned_thread_does_not_inherit_the_parents_remaining_fuel() {
        let meter = Arc::new(FuelMeter::new(1_000));
        let _scope = meter.begin();
        meter
            .consume(999)
            .expect("one unit should be left on this thread");

        let child = Arc::clone(&meter);
        std::thread::spawn(move || {
            // Without `arm_thread`, a new thread starts on `DEFAULT_FUEL`.
            child
                .consume(DEFAULT_FUEL)
                .expect("a fresh thread starts from DEFAULT_FUEL");
            assert_eq!(child.consume(1).unwrap_err(), Exhausted);
        })
        .join()
        .expect("the child thread panicked");

        // The child's consumption must not affect its parent.
        meter
            .consume(1)
            .expect("the parent still holds its last unit");
        assert_eq!(meter.consume(1).unwrap_err(), Exhausted);
    }

    #[test]
    fn arm_thread_starts_a_thread_on_the_configured_budget() {
        // Use a value distinct from `DEFAULT_FUEL` to detect a missing reset.
        let meter = Arc::new(FuelMeter::new(500));
        let child = Arc::clone(&meter);
        std::thread::spawn(move || {
            child.arm_thread();
            child
                .consume(500)
                .expect("the child starts from the configured budget");
            assert_eq!(child.consume(1).unwrap_err(), Exhausted);
        })
        .join()
        .expect("the child thread panicked");
    }

    /// Metering host that arms each spawned evaluation through `begin_thread_evaluation`.
    #[derive(Clone, Debug)]
    struct FuelCtx {
        fuel: Arc<FuelMeter>,
        logs: Arc<RwLock<Vec<String>>>,
        ticks: Arc<AtomicU32>,
    }

    impl PartialEq for FuelCtx {
        fn eq(&self, other: &Self) -> bool {
            Arc::ptr_eq(&self.fuel, &other.fuel)
        }
    }

    impl LispContext for FuelCtx {
        fn consume_fuel(&self, amount: u32) -> Result<(), EvalError<FuelCtx>> {
            self.fuel.consume(amount).map_err(|_| EvalError::OutOfFuel)
        }

        fn log_diagnostic(&self, msg: &str) {
            self.logs
                .write()
                .expect("log lock poisoned")
                .push(msg.to_string());
        }

        fn begin_thread_evaluation(&self) {
            self.fuel.arm_thread();
        }

        fn with_unwind<R>(&self, cleanup: impl FnOnce() -> R) -> R {
            self.fuel.with_unwind(100, cleanup)
        }
    }

    fn fuel_env(budget: u32) -> (Arc<Env<FuelCtx>>, FuelCtx) {
        let env = Env::new_root();
        setup_base_env(env.clone());
        // Count background iterations to check the child's budget without relying on timing.
        env.set_function(
            "tick!".into(),
            LispExp::primitive(
                |_args, _env, ctx: &FuelCtx| {
                    ctx.ticks.fetch_add(1, Ordering::Relaxed);
                    Ok(LispExp::nil())
                },
                None,
            ),
        );
        let ctx = FuelCtx {
            fuel: Arc::new(FuelMeter::new(budget)),
            logs: Arc::new(RwLock::new(Vec::new())),
            ticks: Arc::new(AtomicU32::new(0)),
        };
        (env, ctx)
    }

    fn eval_script(
        script: &str,
        env: Arc<Env<FuelCtx>>,
        ctx: &FuelCtx,
    ) -> Result<LispExp<FuelCtx>, EvalError<FuelCtx>> {
        let wrapped = format!("(progn {script})");
        let mut parser = Parser::new(&wrapped);
        eval(
            &parser.next().expect("failed to parse test script"),
            env,
            ctx,
        )
    }

    #[test]
    fn unwind_protect_does_not_refill_normal_evaluation() {
        let (env, ctx) = fuel_env(50);
        let _scope = ctx.fuel.begin();

        assert_eq!(
            eval_script("(dotimes (i 1000) (unwind-protect nil nil))", env, &ctx,),
            Err(EvalError::OutOfFuel),
        );
    }

    #[test]
    fn unwind_protect_runs_cleanup_after_exhaustion() {
        let (env, ctx) = fuel_env(20);
        env.set_variable("cleaned".into(), LispExp::nil());

        let _scope = ctx.fuel.begin();

        assert_eq!(
            eval_script(
                "(unwind-protect
                     (while t nil)
                     (setq cleaned t))",
                env.clone(),
                &ctx,
            ),
            Err(EvalError::OutOfFuel),
        );

        assert_eq!(env.get_variable("cleaned"), Some(LispExp::t()),);

        assert_eq!(ctx.fuel.consume(1), Err(Exhausted));
    }

    #[test]
    fn unwind_protect_runs_cleanup_after_last_ordinary_unit() {
        // progn, unwind-protect and nil consume three ordinary units.
        let (env, ctx) = fuel_env(3);
        env.set_variable("cleaned".into(), LispExp::nil());

        let _scope = ctx.fuel.begin();

        assert_eq!(
            eval_script("(unwind-protect nil (setq cleaned t))", env.clone(), &ctx,),
            Ok(LispExp::nil()),
        );

        assert_eq!(env.get_variable("cleaned"), Some(LispExp::t()),);

        assert_eq!(ctx.fuel.consume(1), Err(Exhausted));
    }

    #[test]
    fn nested_unwind_protect_cannot_refill_the_reserve() {
        let (env, ctx) = fuel_env(20);
        let _scope = ctx.fuel.begin();

        assert_eq!(
            eval_script(
                "(unwind-protect
                     (while t nil)
                     (tick!)
                     (unwind-protect
                         (while t nil)
                         (tick!)))",
                env,
                &ctx,
            ),
            Err(EvalError::OutOfFuel),
        );

        assert_eq!(ctx.ticks.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn unwind_reserve_is_shared_between_cleanups() {
        let meter = FuelMeter::new(20);
        let _scope = meter.begin();

        // Ordinary cleanup work consumes ordinary fuel.
        meter.with_unwind(10, || meter.consume(7).unwrap());
        meter.consume(13).unwrap();

        assert_eq!(meter.consume(1), Err(Exhausted));

        // First cleanup spends seven reserve units.
        meter.with_unwind(10, || meter.consume(7).unwrap());

        // Reserve fuel is unavailable outside cleanup.
        assert_eq!(meter.consume(1), Err(Exhausted));

        // The next cleanup receives only the remaining three.
        meter.with_unwind(10, || {
            meter.consume(3).unwrap();
            assert_eq!(meter.consume(1), Err(Exhausted));
        });
    }

    /// Wait for the effect because `spawn` does not join. The timeout also allows
    /// an incorrectly armed thread to exhaust `DEFAULT_FUEL`, so the test can
    /// report the budget mismatch.
    fn wait_for(ctx: &FuelCtx, needle: &str) -> bool {
        for _ in 0..2_000 {
            if ctx
                .logs
                .read()
                .expect("log lock poisoned")
                .iter()
                .any(|line| line.contains(needle))
            {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn a_runaway_spawned_thread_is_stopped_by_its_own_budget() {
        let (env, ctx) = fuel_env(50_000);
        eval_script("(spawn (lambda () (while t 1)))", env, &ctx)
            .expect("spawning should succeed on the parent");
        assert!(
            wait_for(&ctx, "OutOfFuel"),
            "the child looped without ever being charged -- a spawned thread is unmetered"
        );
    }

    #[test]
    fn a_childs_exhaustion_leaves_the_parent_free_to_keep_running() {
        let (env, ctx) = fuel_env(30_000);
        eval_script("(spawn (lambda () (while t 1)))", env.clone(), &ctx).expect("spawn failed");
        assert!(
            wait_for(&ctx, "OutOfFuel"),
            "the child never reported running out of fuel"
        );

        // Exhausting the child's allowance must not consume the parent's fuel.
        eval_script("(setq i 0) (while (< i 500) (setq i (+ i 1)))", env, &ctx)
            .expect("the child's exhaustion must not touch the parent's counter");
    }

    #[test]
    fn a_spawned_thread_is_armed_from_the_meter_not_the_compile_time_default() {
        // A small configured budget makes a missing thread reset visible in iteration counts.
        let (env, ctx) = fuel_env(2_000);
        eval_script("(spawn (lambda () (while t (tick!))))", env, &ctx).expect("spawn failed");
        assert!(
            wait_for(&ctx, "OutOfFuel"),
            "the child never reported running out of fuel"
        );

        let ticks = ctx.ticks.load(Ordering::Relaxed);
        assert!(
            ticks < 2_000,
            "the child got through {ticks} iterations on a 2000-step budget -- it was left on \
             DEFAULT_FUEL instead of being armed from the meter"
        );
    }

    #[test]
    fn dotimes_with_empty_body_respects_fuel() {
        let (env, ctx) = fuel_env(10);
        let _scope = ctx.fuel.begin();

        assert_eq!(
            eval_script("(dotimes (i 1000))", env, &ctx),
            Err(EvalError::OutOfFuel)
        );
    }

    #[test]
    fn dotimes_completes_with_enough_fuel() {
        let (env, ctx) = fuel_env(100);
        let _scope = ctx.fuel.begin();

        let result = eval_script(
            "(setq total 0)
             (dotimes (i 3)
               (setq total (+ total 1)))
             total",
            env,
            &ctx,
        );

        assert_eq!(result, Ok(LispExp::number(3.0)));
    }

    #[test]
    fn dolist_with_empty_body_respects_fuel() {
        let (env, ctx) = fuel_env(10);
        env.set_variable(
            "items".into(),
            LispExp::proper_list(vec![LispExp::number(1.0); 1000]),
        );

        let _scope = ctx.fuel.begin();

        assert_eq!(
            eval_script("(dolist (item items))", env, &ctx),
            Err(EvalError::OutOfFuel)
        );
    }

    #[test]
    fn dolist_completes_with_enough_fuel() {
        let (env, ctx) = fuel_env(100);
        let _scope = ctx.fuel.begin();

        let result = eval_script(
            "(setq total 0)
             (dolist (item '(1 2 3))
               (setq total (+ total item)))
             total",
            env,
            &ctx,
        );

        assert_eq!(result, Ok(LispExp::number(6.0)));
    }
}
