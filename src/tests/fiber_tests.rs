#[cfg(test)]
mod tests {
    use crate::base::setup_base_env;
    use crate::{Env, EvalError, LispContext, LispExp, Parser, eval};
    use std::sync::Arc;

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

    fn setup_fiber_env() -> (Arc<Env<()>>, ()) {
        let env = Env::new_root();

        setup_base_env(env.clone());

        (env, ())
    }

    #[test]
    fn test_fiber_basic_yielding() {
        let (env, mut ctx) = setup_fiber_env();

        // Explicit yields separate the three execution steps.
        let script = r#"
            (setq my-task (fiber
                (yield 10.0)
                (yield 20.0)
                30.0))
        "#;
        eval_script(script, env.clone(), &mut ctx).unwrap();

        let step1 = eval_script("(resume my-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(step1, LispExp::Number(10.0));

        let step2 = eval_script("(resume my-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(step2, LispExp::Number(20.0));

        let step3 = eval_script("(resume my-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(step3, LispExp::Number(30.0));

        let exhausted1 = eval_script("(resume my-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(exhausted1, LispExp::symbol("nil".into()));

        let exhausted2 = eval_script("(resume my-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(exhausted2, LispExp::symbol("nil".into()));
    }

    #[test]
    fn test_fiber_internal_state_preservation() {
        let (env, mut ctx) = setup_fiber_env();

        // The continuation must retain the `let` binding across yields.
        let script = r#"
            (setq counter-task
                (let ((counter 0))
                    (fiber
                        (yield (setq counter (+ counter 1)))
                        (yield (setq counter (+ counter 10)))
                        (setq counter (+ counter 100)))))
        "#;

        env.set_function(
            "+".into(),
            LispExp::primitive(
                |args, _, _| {
                    let mut sum = 0.0;
                    for arg in args {
                        if let LispExp::Number(n) = arg {
                            sum += n;
                        }
                    }
                    Ok(LispExp::Number(sum))
                },
                None,
            ),
        );

        eval_script(script, env.clone(), &mut ctx).unwrap();

        let step1 = eval_script("(resume counter-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(step1, LispExp::Number(1.0));

        let step2 = eval_script("(resume counter-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(step2, LispExp::Number(11.0));

        let step3 = eval_script("(resume counter-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(step3, LispExp::Number(111.0));
    }

    #[test]
    fn test_multiple_interleaved_fibers() {
        let (env, mut ctx) = setup_fiber_env();

        let script = r#"
            (setq task-a (fiber (yield "A1") "A2"))
            (setq task-b (fiber (yield "B1") "B2"))
        "#;
        eval_script(script, env.clone(), &mut ctx).unwrap();

        let a1 = eval_script("(resume task-a)", env.clone(), &mut ctx).unwrap();
        assert_eq!(a1, LispExp::string("A1".into()));

        let b1 = eval_script("(resume task-b)", env.clone(), &mut ctx).unwrap();
        assert_eq!(b1, LispExp::string("B1".into()));

        let a2 = eval_script("(resume task-a)", env.clone(), &mut ctx).unwrap();
        assert_eq!(a2, LispExp::string("A2".into()));

        let b2 = eval_script("(resume task-b)", env.clone(), &mut ctx).unwrap();
        assert_eq!(b2, LispExp::string("B2".into()));
    }

    #[test]
    fn test_fiber_empty_instantiation() {
        let (env, mut ctx) = setup_fiber_env();

        // An empty fiber is already complete.
        eval_script("(setq empty-task (fiber))", env.clone(), &mut ctx).unwrap();

        let res = eval_script("(resume empty-task)", env.clone(), &mut ctx).unwrap();
        assert_eq!(res, LispExp::symbol("nil".into()));
    }
}
