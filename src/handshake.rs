use crate::{Env, EvalError, LispContext, LispExp, Parser, eval, setup_base_env};
use std::sync::Arc;

/// Create a standard environment and verify arithmetic and fiber suspension.
///
/// The diagnostic script leaves `verify-math`, `verify-fiber`, and `fiber-step`
/// bound in the returned environment. Evaluation failures propagate; a parse
/// or verification failure returns [`EvalError::UncorrectFunctionDefinition`].
/// Diagnostics are sent through [`LispContext::log_diagnostic`].
pub fn bootstrap_vm<T: LispContext>(ctx: &T) -> Result<Arc<Env<T>>, EvalError<T>> {
    let env = Env::new_root();

    setup_base_env(env.clone());

    let verification_script = r#"
        (progn
            ;; Test structural scoping & primitives
            (setq verify-math (- (+ 5 5) 2)) ;; 8
            
            ;; Test Fiber allocation & suspension: the first `resume' has to
            ;; stop at the `yield' and hand its value back, leaving the 200
            ;; for a second one. A fiber that ran straight through would
            ;; answer 200 here, and one that never resumed would answer nil.
            (setq verify-fiber (fiber (yield 100) 200))
            (setq fiber-step (resume verify-fiber))
            
            ;; Return validation status
            (if (= verify-math 8)
                (if (= fiber-step 100)
                    'vm-status-healthy
                    'error-fiber-failed)
                'error-math-failed))
    "#;

    let mut parser = Parser::new(verification_script);
    let ast = parser.next().or_else(|_| {
        Err(EvalError::UncorrectFunctionDefinition)
    })?;

    ctx.log_diagnostic("VM Handshake: Initiating verification script...");

    let status = eval(&ast, env.clone(), ctx)?;

    if status == LispExp::symbol("vm-status-healthy".into()) {
        ctx.log_diagnostic("VM Handshake: State verification successful. Core is stable.");
        Ok(env)
    } else {
        ctx.log_diagnostic(&format!(
            "VM Handshake CRITICAL: State corrupted. Got {:?}",
            status
        ));
        Err(EvalError::UncorrectFunctionDefinition)
    }
}
