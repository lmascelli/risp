//! Fiber suspension and resumption: statement order, captured scope, repeated
//! yields, and rejection before argument effects in unsupported positions.
#[cfg(test)]
mod tests {
    use crate::base::setup_base_env;
    use crate::{Env, EvalError, LispContext, LispExp, Parser, eval};
    use std::sync::{Arc, RwLock};

    /// A host that remembers, in order, everything the program logged.
    #[derive(Clone, Debug)]
    struct LogCtx {
        lines: Arc<RwLock<Vec<String>>>,
    }

    impl LogCtx {
        fn new() -> Self {
            Self {
                lines: Arc::new(RwLock::new(Vec::new())),
            }
        }

        fn lines(&self) -> Vec<String> {
            self.lines.read().expect("log").clone()
        }
    }

    impl LispContext for LogCtx {
        fn consume_fuel(&self, _amount: u32) -> Result<(), EvalError<LogCtx>> {
            Ok(())
        }
        fn log_diagnostic(&self, msg: &str) {
            self.lines.write().expect("log").push(msg.to_string());
        }
    }

    impl PartialEq for LogCtx {
        fn eq(&self, other: &Self) -> bool {
            Arc::ptr_eq(&self.lines, &other.lines)
        }
    }

    /// Append the argument text to the host's ordered log.
    fn log_primitive(
        args: &[LispExp<LogCtx>],
        _env: Arc<Env<LogCtx>>,
        ctx: &LogCtx,
    ) -> Result<LispExp<LogCtx>, EvalError<LogCtx>> {
        if let Some(LispExp::String(text)) = args.first() {
            ctx.log_diagnostic(text);
        }
        Ok(LispExp::nil())
    }

    fn setup() -> (Arc<Env<LogCtx>>, LogCtx) {
        let env = Env::new_root();
        setup_base_env(env.clone());
        env.set_function("log".into(), LispExp::primitive(log_primitive, None));
        (env, LogCtx::new())
    }

    fn run(script: &str, env: &Arc<Env<LogCtx>>, ctx: &LogCtx) -> LispExp<LogCtx> {
        try_run(script, env, ctx).unwrap_or_else(|why| panic!("evaluating {script}: {why:?}"))
    }

    fn try_run(
        script: &str,
        env: &Arc<Env<LogCtx>>,
        ctx: &LogCtx,
    ) -> Result<LispExp<LogCtx>, EvalError<LogCtx>> {
        let wrapped = format!("(progn {script})");
        let exp = Parser::new(&wrapped).next().expect("the test must parse");
        eval(&exp, env.clone(), ctx)
    }

    fn number(value: f64) -> LispExp<LogCtx> {
        LispExp::Number(value)
    }

    // Stopping inside a form

    #[test]
    fn a_fiber_stops_at_the_yield_and_carries_on_from_there() {
        // Each side of the yield must run exactly once, on separate resumes.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (log "a") (yield 1) (log "b") 2)))"#,
            &env,
            &ctx,
        );
        assert_eq!(run("(resume f)", &env, &ctx), number(1.0));
        assert_eq!(ctx.lines(), vec!["a"]);
        assert_eq!(run("(resume f)", &env, &ctx), number(2.0));
        assert_eq!(ctx.lines(), vec!["a", "b"]);
    }

    #[test]
    fn a_bare_yield_hands_back_nil() {
        let (env, ctx) = setup();
        run(r#"(setq f (fiber (progn (yield) 7)))"#, &env, &ctx);
        assert!(run("(resume f)", &env, &ctx).is_nil());
        assert_eq!(run("(resume f)", &env, &ctx), number(7.0));
    }

    #[test]
    fn the_fiber_is_not_done_while_it_is_merely_suspended() {
        // A yield in the last form suspends the fiber; it does not complete it.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (yield 1) (log "after") 2)))"#,
            &env,
            &ctx,
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(run("(resume f)", &env, &ctx), number(2.0));
        assert_eq!(ctx.lines(), vec!["after"]);
        assert!(
            run("(resume f)", &env, &ctx).is_nil(),
            "and now it really is finished"
        );
    }

    // Loop continuations.

    #[test]
    fn a_loop_resumes_in_the_middle_of_the_iteration_it_stopped_in() {
        // Resuming at the top would repeat the first log entry and skip the second.
        let (env, ctx) = setup();
        run(
            r#"(setq n 0)
               (setq f (fiber (while (< n 3)
                                (log "one")
                                (yield n)
                                (log "two")
                                (setq n (+ n 1)))))"#,
            &env,
            &ctx,
        );
        assert_eq!(run("(resume f)", &env, &ctx), number(0.0));
        assert_eq!(ctx.lines(), vec!["one"]);
        assert_eq!(run("(resume f)", &env, &ctx), number(1.0));
        assert_eq!(ctx.lines(), vec!["one", "two", "one"]);
        assert_eq!(run("(resume f)", &env, &ctx), number(2.0));
        assert_eq!(ctx.lines(), vec!["one", "two", "one", "two", "one"]);
    }

    #[test]
    fn a_loop_that_yields_every_turn_stays_inside_its_loop() {
        // A second yield must retain outer frames that have not yet resumed.
        let (env, ctx) = setup();
        run(
            r#"(setq n 0)
               (setq f (fiber (while t (setq n (+ n 1)) (yield n))))"#,
            &env,
            &ctx,
        );
        for expected in 1..=5 {
            assert_eq!(run("(resume f)", &env, &ctx), number(expected as f64));
        }
    }

    #[test]
    fn a_loop_runs_to_the_end_once_nothing_stops_it() {
        let (env, ctx) = setup();
        run(
            r#"(setq n 0)
               (setq f (fiber (while (< n 3) (yield n) (setq n (+ n 1)))))"#,
            &env,
            &ctx,
        );
        run("(resume f)", &env, &ctx);
        run("(resume f)", &env, &ctx);
        run("(resume f)", &env, &ctx);
        // The fourth resume finishes the last iteration, finds the condition
        // false and falls out of the loop.
        run("(resume f)", &env, &ctx);
        assert_eq!(run("n", &env, &ctx), number(3.0));
        assert!(run("(resume f)", &env, &ctx).is_nil());
    }

    // Scope

    #[test]
    fn a_let_binding_survives_the_suspension_it_was_made_before() {
        // After resumption, read the captured binding rather than an enclosing shadow.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (let ((x 10)) (yield) (setq x (+ x 5)) x)))"#,
            &env,
            &ctx,
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(run("(resume f)", &env, &ctx), number(15.0));
    }

    #[test]
    fn a_shadowed_binding_is_still_the_inner_one_after_resuming() {
        let (env, ctx) = setup();
        run(
            r#"(setq x 1)
               (setq f (fiber (let ((x 99)) (yield) x)))"#,
            &env,
            &ctx,
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(run("(resume f)", &env, &ctx), number(99.0));
        assert_eq!(
            run("x", &env, &ctx),
            number(1.0),
            "the outer one is untouched"
        );
    }

    #[test]
    fn a_yield_inside_a_called_function_suspends_the_call() {
        let (env, ctx) = setup();
        run(
            r#"(defun step-twice () (log "first") (yield 'half) (log "second"))
               (setq f (fiber (progn (step-twice) 'whole)))"#,
            &env,
            &ctx,
        );
        assert_eq!(
            run("(resume f)", &env, &ctx),
            LispExp::symbol("half".into())
        );
        assert_eq!(ctx.lines(), vec!["first"]);
        assert_eq!(
            run("(resume f)", &env, &ctx),
            LispExp::symbol("whole".into())
        );
        assert_eq!(ctx.lines(), vec!["first", "second"]);
    }

    #[test]
    fn nested_blocks_resume_from_the_inside_out() {
        // Each nested frame has work remaining, exposing incorrect replay order.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn
                                (let ((x 1))
                                  (progn (yield) (log "inner"))
                                  (log "middle"))
                                (log "outer"))))"#,
            &env,
            &ctx,
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(ctx.lines(), Vec::<String>::new());
        run("(resume f)", &env, &ctx);
        assert_eq!(ctx.lines(), vec!["inner", "middle", "outer"]);
    }

    #[test]
    fn a_yield_in_tail_position_is_allowed() {
        // A tail call can yield because its enclosing statement records the continuation.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (when t (yield 'parked)) (log "resumed"))))"#,
            &env,
            &ctx,
        );
        assert_eq!(
            run("(resume f)", &env, &ctx),
            LispExp::symbol("parked".into())
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(ctx.lines(), vec!["resumed"]);
    }

    // The boundary

    #[test]
    fn a_yield_in_an_argument_is_refused() {
        // Coming back here would mean delivering a value into a call that was
        // half-built and is now gone.
        let (env, ctx) = setup();
        run(r#"(setq f (fiber (progn (+ 1 (yield)))))"#, &env, &ctx);
        assert_eq!(
            try_run("(resume f)", &env, &ctx).unwrap_err(),
            EvalError::YieldNotAllowed
        );
    }

    #[test]
    fn a_yield_in_a_condition_is_refused() {
        let (env, ctx) = setup();
        run(r#"(setq f (fiber (progn (if (yield) 1 2))))"#, &env, &ctx);
        assert_eq!(
            try_run("(resume f)", &env, &ctx).unwrap_err(),
            EvalError::YieldNotAllowed
        );
    }

    #[test]
    fn a_yield_inside_a_protected_form_is_refused() {
        // Unwind protection cannot be suspended without retaining its cleanup obligation.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (unwind-protect (yield) (log "cleanup")))))"#,
            &env,
            &ctx,
        );
        assert_eq!(
            try_run("(resume f)", &env, &ctx).unwrap_err(),
            EvalError::YieldNotAllowed
        );
    }

    #[test]
    fn a_yield_inside_a_condition_case_is_refused() {
        // The handler lives on the Rust stack, so suspension must be rejected as an error.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (condition-case e (yield) (error (log "handled"))))))"#,
            &env,
            &ctx,
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(ctx.lines(), vec!["handled"]);
        assert!(
            run("(resume f)", &env, &ctx).is_nil(),
            "nothing was left suspended"
        );
    }

    #[test]
    fn a_yield_inside_a_dolist_is_refused() {
        // `dolist` has no continuation frame; use `while` for iteration across yields.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (dolist (x '(1 2 3)) (yield x)))))"#,
            &env,
            &ctx,
        );
        assert_eq!(
            try_run("(resume f)", &env, &ctx).unwrap_err(),
            EvalError::YieldNotAllowed
        );
    }

    #[test]
    fn a_yield_from_inside_a_primitive_callback_is_refused() {
        // A native `mapcar` loop cannot record its partially collected results.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (mapcar (lambda (x) (yield x)) '(1 2)))))"#,
            &env,
            &ctx,
        );
        assert_eq!(
            try_run("(resume f)", &env, &ctx).unwrap_err(),
            EvalError::YieldNotAllowed
        );
    }

    #[test]
    fn a_yield_outside_a_fiber_is_refused() {
        let (env, ctx) = setup();
        assert_eq!(
            try_run("(yield 1)", &env, &ctx).unwrap_err(),
            EvalError::YieldNotAllowed
        );
    }

    #[test]
    fn a_refused_yield_does_not_run_its_own_argument_first() {
        // Reject the yield before its argument performs observable effects.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (+ 1 (yield (log "side-effect"))))))"#,
            &env,
            &ctx,
        );
        assert!(try_run("(resume f)", &env, &ctx).is_err());
        assert_eq!(ctx.lines(), Vec::<String>::new());
    }

    // Fiber completion and re-entry.

    #[test]
    fn a_fiber_that_never_yields_runs_to_the_end_in_one_resume() {
        // Only explicit `yield` suspends a fiber; adjacent body forms run in one turn.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (log "a") (log "b") (log "c")))"#,
            &env,
            &ctx,
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(ctx.lines(), vec!["a", "b", "c"]);
        assert!(run("(resume f)", &env, &ctx).is_nil());
    }

    #[test]
    fn fiber_done_p_tells_a_finished_fiber_from_a_parked_one() {
        // A yielded nil does not indicate completion; only `fiber-done-p` does.
        let (env, ctx) = setup();
        run(r#"(setq f (fiber (progn (yield) nil)))"#, &env, &ctx);
        assert!(run("(fiber-done-p f)", &env, &ctx).is_nil(), "not started");
        assert!(run("(resume f)", &env, &ctx).is_nil(), "yielded nil");
        assert!(
            run("(fiber-done-p f)", &env, &ctx).is_nil(),
            "suspended, which is not finished"
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(run("(fiber-done-p f)", &env, &ctx), LispExp::t());
    }

    #[test]
    fn fiber_done_p_refuses_anything_that_is_not_a_fiber() {
        let (env, ctx) = setup();
        assert!(try_run("(fiber-done-p 1)", &env, &ctx).is_err());
    }

    #[test]
    fn a_fiber_is_worth_the_value_of_its_last_form() {
        // Empty outer frames must preserve the result produced by an inner frame.
        let (env, ctx) = setup();
        run(r#"(setq f (fiber (progn (yield) 7)))"#, &env, &ctx);
        run("(resume f)", &env, &ctx);
        assert_eq!(run("(resume f)", &env, &ctx), number(7.0));
    }

    #[test]
    fn a_suspended_form_finishes_before_the_rest_of_the_body_runs() {
        // After the final yield, finish the inner form and continue the outer body.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (yield 1) (yield 2) (log "end of first"))
                              (log "second")))"#,
            &env,
            &ctx,
        );
        assert_eq!(run("(resume f)", &env, &ctx), number(1.0));
        assert_eq!(run("(resume f)", &env, &ctx), number(2.0));
        run("(resume f)", &env, &ctx);
        assert_eq!(ctx.lines(), vec!["end of first", "second"]);
        assert!(run("(resume f)", &env, &ctx).is_nil());
    }

    #[test]
    fn an_error_inside_a_fiber_ends_it() {
        // An error discards the continuation; resuming again must not replay completed work.
        let (env, ctx) = setup();
        run(
            r#"(setq f (fiber (progn (yield 1) (undefined-fn) (log "never"))))"#,
            &env,
            &ctx,
        );
        run("(resume f)", &env, &ctx);
        assert_eq!(
            try_run("(resume f)", &env, &ctx).unwrap_err(),
            EvalError::UndefinedFunction("undefined-fn".into())
        );
        assert!(run("(resume f)", &env, &ctx).is_nil());
        assert_eq!(ctx.lines(), Vec::<String>::new());
    }

    #[test]
    fn a_fiber_may_be_resumed_from_inside_lisp_that_the_fiber_itself_started() {
        // Re-entering a running fiber must not deadlock on a lock held by `resume`.
        let (env, ctx) = setup();
        run(
            r#"(setq inner (fiber (progn (log "inner") (yield))))
               (setq outer (fiber (progn (resume inner) (log "outer"))))"#,
            &env,
            &ctx,
        );
        run("(resume outer)", &env, &ctx);
        assert_eq!(ctx.lines(), vec!["inner", "outer"]);
    }
    #[test]
    fn the_documented_cooperative_scheduler_runs() {
        // Run the cooperative scheduler example from `doc/lisp-fibers.typ`.
        let (env, ctx) = setup();
        run(
            r#"
            (setq scheduler--tasks nil)

            (defun spawn-task (f)
              (setq scheduler--tasks (append scheduler--tasks (list f))))

            (defun scheduler-tick ()
              (let ((live nil))
                (mapc (lambda (f)
                        (resume f)
                        (if (fiber-done-p f) nil (setq live (append live (list f)))))
                      scheduler--tasks)
                (setq scheduler--tasks live)
                (length live)))
            "#,
            &env,
            &ctx,
        );
        run(
            r#"(spawn-task (fiber (progn (log "a1") (yield) (log "a2"))))
               (spawn-task (fiber (progn (log "b1") (yield) (log "b2") (yield) (log "b3"))))"#,
            &env,
            &ctx,
        );
        assert_eq!(run("(scheduler-tick)", &env, &ctx), number(2.0));
        assert_eq!(ctx.lines(), vec!["a1", "b1"]);
        assert_eq!(
            run("(scheduler-tick)", &env, &ctx),
            number(1.0),
            "the shorter task finished and was retired"
        );
        assert_eq!(ctx.lines(), vec!["a1", "b1", "a2", "b2"]);
        assert_eq!(run("(scheduler-tick)", &env, &ctx), number(0.0));
        assert_eq!(ctx.lines(), vec!["a1", "b1", "a2", "b2", "b3"]);
    }
}
