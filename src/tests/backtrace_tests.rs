//! Tests for retained failure frames, successful pops, and handled-error cleanup.
#[cfg(test)]
mod tests {
    use crate::{Env, EvalError, LispContext, LispExp, Parser, eval, setup_base_env};
    use std::sync::RwLock;

    #[derive(Debug)]
    struct BacktraceCtx {
        call_stack: RwLock<Vec<String>>,
    }

    impl Clone for BacktraceCtx {
        fn clone(&self) -> Self {
            unreachable!()
        }
    }

    impl PartialEq for BacktraceCtx {
        fn eq(&self, _other: &Self) -> bool {
            unreachable!()
        }
    }

    impl LispContext for BacktraceCtx {
        fn consume_fuel(&self, _amount: u32) -> Result<(), EvalError<BacktraceCtx>> {
            Ok(())
        }
        fn log_diagnostic(&self, _msg: &str) {}
        fn push_call_frame(&self, frame: &str) {
            self.call_stack.write().unwrap().push(frame.to_string());
        }
        fn pop_call_frame(&self) {
            self.call_stack.write().unwrap().pop();
        }
        fn call_frame_depth(&self) -> usize {
            self.call_stack.read().unwrap().len()
        }
        fn truncate_call_frames(&self, depth: usize) {
            self.call_stack.write().unwrap().truncate(depth);
        }
    }

    impl BacktraceCtx {
        fn new() -> Self {
            Self {
                call_stack: RwLock::new(Vec::new()),
            }
        }

        /// Most recent call first.
        fn backtrace(&self) -> Vec<String> {
            let mut frames = self.call_stack.read().unwrap().clone();
            frames.reverse();
            frames
        }
    }

    fn eval_script(
        ctx: &BacktraceCtx,
        script: &str,
    ) -> Result<LispExp<BacktraceCtx>, EvalError<BacktraceCtx>> {
        let env = Env::new_root();
        setup_base_env(env.clone());
        let wrapped = format!("(progn {})", script);
        let mut parser = Parser::new(&wrapped);
        let ast = parser.next().expect("failed to parse test script");
        eval(&ast, env, ctx)
    }

    #[test]
    fn a_successful_call_leaves_the_stack_empty() {
        let ctx = BacktraceCtx::new();
        eval_script(&ctx, "(defun f (x) (+ x 1)) (f 41)").unwrap();
        assert_eq!(ctx.backtrace(), Vec::<String>::new());
    }

    #[test]
    fn a_non_tail_call_stays_on_a_frozen_stack() {
        // Only `f` survives: its `(g)` call is not in tail position, while `g` and
        // `h` replace their frames with tail calls.
        let ctx = BacktraceCtx::new();
        let result = eval_script(
            &ctx,
            "(defun h () (undefined-fn))
             (defun g () (h))
             (defun f () (g) 1)
             (f)",
        );
        assert_eq!(
            result.unwrap_err(),
            EvalError::UndefinedFunction("undefined-fn".into())
        );
        assert_eq!(ctx.backtrace(), vec!["f"]);
    }

    #[test]
    fn a_primitive_that_evaluates_its_own_nested_expression_stays_on_the_stack() {
        // The primitive remains on the stack while its nested evaluation fails.
        let ctx = BacktraceCtx::new();
        let result = eval_script(&ctx, r#"(eval-string "(1 2 3)")"#);
        assert_eq!(result.unwrap_err(), EvalError::UnvalidFunctionCall);
        assert_eq!(ctx.backtrace(), vec!["eval-string"]);
    }

    #[test]
    fn funcall_stays_on_the_stack_across_the_function_it_invokes() {
        // `funcall` remains active while the lambda's final form is tail-called.
        let ctx = BacktraceCtx::new();
        let result = eval_script(
            &ctx,
            r#"(defun ee (input) (eval-string input))
               (funcall 'ee "(1 2 3)")"#,
        );
        assert_eq!(result.unwrap_err(), EvalError::UnvalidFunctionCall);
        assert_eq!(ctx.backtrace(), vec!["eval-string", "funcall"]);
    }

    #[test]
    fn eval_string_safe_does_not_leak_frames_from_the_error_it_swallows() {
        // Handled failures must not leave frames in a later uncaught error's backtrace.
        let ctx = BacktraceCtx::new();
        eval_script(&ctx, r#"(eval-string-safe "(eval-string \"(1 2 3)\")")"#).unwrap();
        assert_eq!(LispContext::call_frame_depth(&ctx), 0);
        assert_eq!(ctx.backtrace(), Vec::<String>::new());

        let result = eval_script(&ctx, r#"(defun f () (eval-string "(1 2 3)")) (f)"#);
        assert_eq!(result.unwrap_err(), EvalError::UnvalidFunctionCall);
        assert_eq!(ctx.backtrace(), vec!["eval-string"]);
    }
}
