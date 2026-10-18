#[cfg(test)]
mod tests {
    use crate::base::setup_base_env;
    use crate::{Env, EvalError, LispContext, LispExp, Parser, eval};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    #[derive(Clone, Debug)]
    struct ThreadCtx {
        pub host_counter: Arc<RwLock<usize>>,
    }

    impl LispContext for ThreadCtx {
        fn consume_fuel(&self, _amount: u32) -> Result<(), EvalError<ThreadCtx>> {
            Ok(())
        }

        fn log_diagnostic(&self, _msg: &str) {}
    }

    impl PartialEq for ThreadCtx {
        fn eq(&self, other: &Self) -> bool {
            Arc::ptr_eq(&self.host_counter, &other.host_counter)
        }
    }

    fn eval_script<T: LispContext>(
        script: &str,
        env: Arc<Env<T>>,
        ctx: &mut T,
    ) -> Result<LispExp<T>, EvalError<T>> {
        let wrapped = format!("(progn {})", script);
        let mut parser = Parser::new(&wrapped);
        let exp = parser.next().unwrap();
        eval(&exp, env, ctx)
    }

    fn setup_thread_env() -> (Arc<Env<ThreadCtx>>, ThreadCtx) {
        let env = Env::new_root();

        setup_base_env(env.clone());

        env.set_function(
            "sleep-ms".into(),
            LispExp::primitive(
                |args, _, _ctx| {
                    if let Some(LispExp::Number(ms)) = args.first() {
                        std::thread::sleep(Duration::from_millis(*ms as u64));
                    }
                    Ok(LispExp::form(vec![]))
                },
                None,
            ),
        );

        env.set_function(
            "bump-host!".into(),
            LispExp::primitive(
                |_args, _, ctx: &ThreadCtx| {
                    let mut lock = ctx.host_counter.write().unwrap();
                    *lock += 1;
                    Ok(LispExp::number(*lock as f64))
                },
                None,
            ),
        );

        let ctx = ThreadCtx {
            host_counter: Arc::new(RwLock::new(0)),
        };

        (env, ctx)
    }

    #[test]
    fn test_baseline_atom_operations() {
        let (env, mut ctx) = setup_thread_env();

        let script = r#"
            (setq my-state (make-atom 10.0))
            (reset my-state 99.0)
            (deref my-state)
        "#;

        let result = eval_script(script, env, &mut ctx).unwrap();
        assert_eq!(result, LispExp::Number(99.0));
    }

    #[test]
    fn test_spawn_async_execution_and_synchronization() {
        let (env, mut ctx) = setup_thread_env();

        // The background thread delays its write so the test can observe both states.
        let script = r#"
            (setq shared-val (make-atom 0.0))
            (spawn (lambda ()
                (progn
                    (sleep-ms 50.0)
                    (reset shared-val 42.0))))
        "#;

        eval_script(script, env.clone(), &mut ctx).unwrap();

        // Check the state before the delayed background update.
        let immediate_val = eval_script("(deref shared-val)", env.clone(), &mut ctx).unwrap();
        assert_eq!(immediate_val, LispExp::Number(0.0));

        // Poll for the background effect because `spawn` exposes no join handle.
        let mut timeout = 0;
        loop {
            let current_val = eval_script("(deref shared-val)", env.clone(), &mut ctx).unwrap();
            if current_val == LispExp::Number(42.0) {
                break;
            }

            std::thread::sleep(Duration::from_millis(10));
            timeout += 10;

            if timeout > 1000 {
                panic!("Background thread failed to execute within 1 second!");
            }
        }
    }

    #[test]
    fn test_multiple_threads_mutating_host_context() {
        let (env, mut ctx) = setup_thread_env();

        // Each spawned thread increments the shared host state once.
        let script = r#"
            (defun spawn-worker (delay)
                (spawn (lambda ()
                    (progn
                        (sleep-ms delay)
                        (bump-host!)))))
            
            (spawn-worker 10.0)
            (spawn-worker 20.0)
            (spawn-worker 30.0)
            (spawn-worker 40.0)
            (spawn-worker 50.0)
            (spawn-worker 15.0)
            (spawn-worker 25.0)
            (spawn-worker 35.0)
            (spawn-worker 45.0)
            (spawn-worker 55.0)
        "#;

        eval_script(script, env.clone(), &mut ctx).unwrap();

        // Allow the delayed background calls to finish before checking the counter.
        std::thread::sleep(Duration::from_millis(150));

        let final_count = *ctx.host_counter.read().unwrap();

        assert_eq!(final_count, 10);
    }
}
