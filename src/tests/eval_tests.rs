#[cfg(test)]
mod test {
    use crate::{Env, EvalError, LispContext, LispExp, Parser, eval};
    use std::{collections::HashMap, sync::Arc, sync::RwLock};

    fn setup_env() -> (std::sync::Arc<Env<()>>, ()) {
        let env = Env::new_root();
        env.set_variable("nil".into(), LispExp::form(vec![]));
        env.set_variable("t".into(), LispExp::number(1.0));
        (env, ())
    }

    fn get_var<T: LispContext>(env: &Env<T>, name: &str) -> Option<LispExp<T>>
    where
        T: Clone,
    {
        env.get_variable(name)
    }

    fn get_func<T: LispContext>(env: &Env<T>, name: &str) -> Option<LispExp<T>>
    where
        T: Clone,
    {
        env.get_function(name)
    }

    #[test]
    fn test_dynamic_variable_lookup() {
        let (global, mut _ctx) = setup_env();
        global.set_variable("x".into(), LispExp::number(10.0));
        global.set_variable("y".into(), LispExp::number(20.0));

        let local = Env::new_child(&global);
        local.set_variable("x".into(), LispExp::number(99.0));

        assert_eq!(get_var(&local, "x"), Some(LispExp::number(99.0)));
        assert_eq!(get_var(&local, "y"), Some(LispExp::number(20.0)));
        assert_eq!(get_var(&local, "z"), None);
    }

    #[test]
    fn test_dynamic_function_lookup() {
        let (mut global, mut _ctx) = setup_env();
        global.set_function("add".into(), LispExp::symbol("built-in-add".into()));

        let local = Env::new_child(&mut global as _);

        assert_eq!(
            get_func(&local, "add"),
            Some(LispExp::symbol("built-in-add".into()))
        );
    }

    #[test]
    fn test_eval_defun() {
        let (env, mut ctx) = setup_env();

        // (defun my-func () "hello")
        let defun_exp = LispExp::form(vec![
            LispExp::symbol("defun".into()),
            LispExp::symbol("my-func".into()),
            LispExp::form(vec![]),
            LispExp::string("hello".into()),
        ]);

        eval(&defun_exp, env.clone(), &mut ctx).unwrap();

        let stored_func = env
            .get_function("my-func")
            .expect("Function should be bound");

        if let LispExp::Lambda(_) = stored_func {
        } else {
            panic!("defun did not store a lambda");
        }
    }

    #[test]
    fn test_error_void_variable() {
        let (env, mut ctx) = setup_env();

        let exp = LispExp::symbol("undefined-var".into());
        let result = eval(&exp, env.clone(), &mut ctx);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            EvalError::UnboundVariable("undefined-var".into())
        );
    }

    #[test]
    fn test_error_void_function() {
        let (env, mut ctx) = setup_env();

        // (missing-func 1 2)
        let exp = LispExp::form(vec![
            LispExp::symbol("missing-func".into()),
            LispExp::number(1.0),
        ]);
        let result = eval(&exp, env.clone(), &mut ctx);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            EvalError::UndefinedFunction("missing-func".into())
        );
    }

    #[test]
    fn test_error_calling_variable_as_function() {
        let (env, mut ctx) = setup_env();

        env.set_variable("x".into(), LispExp::number(42.0));

        let exp = LispExp::form(vec![LispExp::symbol("x".into())]);
        let result = eval(&exp, env.clone(), &mut ctx);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            EvalError::UndefinedFunction("x".into())
        );
    }

    #[test]
    fn test_error_invalid_function_call() {
        let (env, mut ctx) = setup_env();

        // (42 "hello") -> The first element is not a symbol!
        let exp = LispExp::form(vec![LispExp::number(42.0), LispExp::string("hello".into())]);
        let result = eval(&exp, env.clone(), &mut ctx);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), EvalError::UnvalidFunctionCall);
    }

    #[test]
    fn test_error_setq_non_symbol() {
        let (env, mut ctx) = setup_env();

        // (setq 42 "value") -> 42 is not a valid variable name
        let exp = LispExp::form(vec![
            LispExp::symbol("setq".into()),
            LispExp::number(42.0),
            LispExp::string("value".into()),
        ]);
        let result = eval(&exp, env.clone(), &mut ctx);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), EvalError::SetqSymbolRequired);
    }

    #[test]
    fn test_error_defun_non_symbol() {
        let (env, mut ctx) = setup_env();

        // (defun "my-func" () 1) -> Function name must be a symbol, not a string
        let exp = LispExp::form(vec![
            LispExp::symbol("defun".into()),
            LispExp::string("my-func".into()),
            LispExp::form(vec![]),
            LispExp::number(1.0),
        ]);
        let result = eval(&exp, env.clone(), &mut ctx);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), EvalError::DefunNameMustBeASymbol);
    }

    #[test]
    fn test_error_if_missing_condition() {
        let (env, mut ctx) = setup_env();

        // (if) -> Missing condition and branches
        let exp = LispExp::form(vec![LispExp::symbol("if".into())]);
        let result = eval(&exp, env.clone(), &mut ctx);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), EvalError::IfNoConditionProvided);
    }

    #[test]
    fn test_eval_robustness_if_missing_branches() {
        let (env, mut ctx) = setup_env();

        // (if t) has a condition but no true branch.
        let exp = LispExp::form(vec![
            LispExp::symbol("if".into()),
            LispExp::symbol("t".into()),
        ]);
        let _ = eval(&exp, env.clone(), &mut ctx);
    }

    #[test]
    fn test_eval_robustness_setq_missing_value() {
        let (env, mut ctx) = setup_env();

        // (setq a) has no value form.
        let exp = LispExp::form(vec![
            LispExp::symbol("setq".into()),
            LispExp::symbol("a".into()),
        ]);
        let _ = eval(&exp, env.clone(), &mut ctx);
    }

    #[test]
    fn test_eval_robustness_defun_missing_args() {
        let (env, mut ctx) = setup_env();

        // (defun my-func) has no parameter list or body.
        let exp = LispExp::form(vec![
            LispExp::symbol("defun".into()),
            LispExp::symbol("my-func".into()),
        ]);
        let _ = eval(&exp, env.clone(), &mut ctx);
    }

    #[test]
    fn test_eval_robustness_malformed_lambda_execution() {
        let (env, mut ctx) = setup_env();

        // Store a non-callable form in the function namespace to test validation.
        let bad_lambda = LispExp::form(vec![LispExp::symbol("lambda".into())]);
        env.set_function("broken-func".into(), bad_lambda);

        let exp = LispExp::form(vec![
            LispExp::symbol("broken-func".into()),
            LispExp::number(10.0),
        ]);

        let result = eval(&exp, env.clone(), &mut ctx);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), EvalError::UncorrectFunctionDefinition);
    }

    #[test]
    fn test_eval_robustness_empty_vector_and_map() {
        let (env, mut ctx) = setup_env();

        // Empty vectors and maps have no elements to evaluate.
        let empty_vec = LispExp::vec(vec![]);
        assert_eq!(
            eval(&empty_vec, env.clone(), &mut ctx).unwrap(),
            LispExp::vec(vec![])
        );
    }

    #[derive(Debug)]
    struct TestHost {
        pub state_changes: RwLock<usize>,
    }

    impl Clone for TestHost {
        fn clone(&self) -> Self {
            unreachable!()
        }
    }
    impl PartialEq for TestHost {
        fn eq(&self, _other: &Self) -> bool {
            unreachable!()
        }
    }

    impl LispContext for TestHost {
        fn consume_fuel(&self, _amount: u32) -> Result<(), EvalError<TestHost>> {
            Ok(())
        }

        fn log_diagnostic(&self, _msg: &str) {}
    }

    fn native_increment_state(
        _args: &[LispExp<TestHost>],
        _env: Arc<Env<TestHost>>,
        ctx: &TestHost,
    ) -> Result<LispExp<TestHost>, EvalError<TestHost>> {
        *ctx.state_changes.write().unwrap() += 1;
        Ok(LispExp::symbol("nil".into()))
    }

    fn native_add(
        args: &[LispExp<TestHost>],
        _env: Arc<Env<TestHost>>,
        _ctx: &TestHost,
    ) -> Result<LispExp<TestHost>, EvalError<TestHost>> {
        let mut sum = 0.0;
        for arg in args {
            if let LispExp::Number(n) = arg {
                sum += n;
            } else {
                return Err(EvalError::WrongArgumentType {
                    expected: "number".into(),
                    got: arg.clone(),
                });
            }
        }
        Ok(LispExp::number(sum))
    }

    fn setup_env_test() -> Arc<Env<TestHost>> {
        let env = Env::new_root();
        env.set_variable("nil".into(), LispExp::form(vec![]));
        env.set_variable("t".into(), LispExp::number(1.0));
        env.set_function("+".into(), LispExp::primitive(native_add, None));
        env.set_function(
            "inc-state".into(),
            LispExp::primitive(native_increment_state, None),
        );
        env
    }

    #[test]
    fn test_lambda_argument_binding() {
        let env = setup_env_test();
        let mut ctx = TestHost {
            state_changes: 0.into(),
        };

        // (defun add-custom (x y) (+ x y))
        let defun_exp = LispExp::form(vec![
            LispExp::symbol("defun".into()),
            LispExp::symbol("add-custom".into()),
            LispExp::form(vec![
                LispExp::symbol("x".into()),
                LispExp::symbol("y".into()),
            ]),
            LispExp::form(vec![
                LispExp::symbol("+".into()),
                LispExp::symbol("x".into()),
                LispExp::symbol("y".into()),
            ]),
        ]);
        eval(&defun_exp, env.clone(), &mut ctx).unwrap();

        // (add-custom 10.0 20.0)
        let call_exp = LispExp::form(vec![
            LispExp::symbol("add-custom".into()),
            LispExp::number(10.0),
            LispExp::number(20.0),
        ]);
        eprintln!("{:?}", call_exp);
        let result = eval(&call_exp, env.clone(), &mut ctx).unwrap();

        assert_eq!(result, LispExp::number(30.0));

        assert!(env.get_variable("x").is_none());
    }

    #[test]
    fn test_generic_host_mutation() {
        let env = setup_env_test();

        let mut ctx = TestHost {
            state_changes: 0.into(),
        };

        // AST: (inc-state)
        let call_mutation = LispExp::form(vec![LispExp::symbol("inc-state".into())]);

        eval(&call_mutation, env.clone(), &mut ctx).unwrap();
        eval(&call_mutation, env.clone(), &mut ctx).unwrap();
        eval(&call_mutation, env.clone(), &mut ctx).unwrap();

        assert_eq!(*ctx.state_changes.read().unwrap(), 3);
    }

    #[derive(Debug)]
    struct MockHost {
        pub tracker: RwLock<f64>,
    }

    impl Clone for MockHost {
        fn clone(&self) -> Self {
            unreachable!()
        }
    }

    impl PartialEq for MockHost {
        fn eq(&self, _other: &Self) -> bool {
            unreachable!()
        }
    }

    impl LispContext for MockHost {
        fn consume_fuel(&self, _amount: u32) -> Result<(), EvalError<MockHost>> {
            Ok(())
        }

        fn log_diagnostic(&self, _msg: &str) {}
    }

    fn setup_interpreter_env() -> (Arc<Env<MockHost>>, MockHost) {
        let env = Env::new_root();

        env.set_function(
            "bump".into(),
            LispExp::primitive(
                |_args: &[LispExp<MockHost>], _env, ctx| {
                    *ctx.tracker.write().unwrap() += 1.0;
                    Ok(LispExp::number(*ctx.tracker.read().unwrap()))
                },
                None,
            ),
        );

        (
            env,
            MockHost {
                tracker: 0.0.into(),
            },
        )
    }

    #[test]
    fn test_progn_execution_and_side_effects() {
        let (env, mut ctx) = setup_interpreter_env();

        // (progn (bump) (bump) 99.0) -> changes state twice, returns last value
        let exp = LispExp::form(vec![
            LispExp::symbol("progn".into()),
            LispExp::form(vec![LispExp::symbol("bump".into())]),
            LispExp::form(vec![LispExp::symbol("bump".into())]),
            LispExp::number(99.0),
        ]);

        let result = eval(&exp, env.clone(), &mut ctx).unwrap();
        assert_eq!(result, LispExp::number(99.0));
        assert_eq!(*ctx.tracker.read().unwrap(), 2.0);

        let empty_progn = LispExp::form(vec![LispExp::symbol("progn".into())]);
        assert_eq!(
            eval(&empty_progn, env.clone(), &mut ctx).unwrap(),
            LispExp::symbol("nil".into())
        )
    }

    #[test]
    fn test_let_malformed_syntax_errors() {
        let (env, mut ctx) = setup_interpreter_env();

        let no_bindings = LispExp::form(vec![LispExp::symbol("let".into())]);
        assert_eq!(
            eval(&no_bindings, env.clone(), &mut ctx).unwrap_err(),
            EvalError::LetNoBindingsProvided
        );

        let invalid_list = LispExp::form(vec![
            LispExp::symbol("let".into()),
            LispExp::symbol("x".into()),
            LispExp::number(1.0),
        ]);
        assert_eq!(
            eval(&invalid_list, env.clone(), &mut ctx).unwrap_err(),
            EvalError::LetUnvalidBindingList
        );

        let broken_tuple = LispExp::form(vec![
            LispExp::symbol("let".into()),
            LispExp::form(vec![LispExp::form(vec![
                LispExp::symbol("x".into()),
                LispExp::number(1.0),
                LispExp::number(2.0),
            ])]),
            LispExp::symbol("x".into()),
        ]);
        assert_eq!(
            eval(&broken_tuple, env.clone(), &mut ctx).unwrap_err(),
            EvalError::LetUnvalidBindingAt(0)
        );
    }

    #[test]
    fn test_map_evaluation_and_expression_resolution() {
        let (env, mut ctx) = setup_interpreter_env();
        env.set_variable("factor".into(), LispExp::number(5.0));

        // A map structure: { "computed-val" (progn (bump) factor) }
        let mut input_map = HashMap::new();
        input_map.insert(
            "computed-val".to_string(),
            LispExp::form(vec![
                LispExp::symbol("progn".into()),
                LispExp::form(vec![LispExp::symbol("bump".into())]),
                LispExp::symbol("factor".into()),
            ]),
        );

        let map_exp = LispExp::map(input_map);
        let evaluated = eval(&map_exp, env.clone(), &mut ctx).unwrap();

        if let LispExp::Map(output_map) = evaluated {
            assert_eq!(output_map.get("computed-val"), Some(&LispExp::number(5.0)));
            assert_eq!(*ctx.tracker.read().unwrap(), 1.0);
        } else {
            panic!("Evaluation should retain structural map wrapper type");
        }
    }

    fn eval_script(script: &str) -> Result<LispExp<()>, EvalError<()>> {
        let env = Env::new_root();

        // Wrap test scripts in one expression so `eval` sees every top-level form.
        let wrapped = format!("(progn {})", script);
        let mut parser = Parser::new(&wrapped);
        let ast = parser.next().unwrap();

        eval(&ast, env, &())
    }

    #[test]
    fn test_eval_quote_symbol() {
        // Quoting an unbound symbol must not attempt variable lookup.
        let res = eval_script("'unbound-var").unwrap();
        assert_eq!(res, LispExp::symbol("unbound-var".into()));
    }

    #[test]
    fn test_eval_quote_list() {
        // Quoting a list of numbers shouldn't try to call `1` as a function
        let res = eval_script("'(1 2 3)").unwrap();
        assert_eq!(
            res,
            LispExp::proper_list(vec![
                LispExp::number(1.0),
                LispExp::number(2.0),
                LispExp::number(3.0),
            ])
        );
    }

    #[test]
    fn test_eval_quote_prevents_function_execution() {
        // '(+ 1 2) should return the literal list [+, 1, 2], NOT 3.
        let res = eval_script("'(+ 1 2)").unwrap();
        assert_eq!(
            res,
            LispExp::proper_list(vec![
                LispExp::symbol("+".into()),
                LispExp::number(1.0),
                LispExp::number(2.0),
            ])
        );
    }

    #[test]
    fn test_eval_quote_explicit_form() {
        let res = eval_script("(quote explicit-symbol)").unwrap();
        assert_eq!(res, LispExp::symbol("explicit-symbol".into()));
    }

    #[test]
    fn test_eval_quote_arity_errors() {
        let no_args = eval_script("(quote)");
        assert_eq!(no_args, Err(EvalError::QuoteNotOneArgument));

        let too_many_args = eval_script("(quote a b)");
        assert_eq!(too_many_args, Err(EvalError::QuoteNotOneArgument));

        let sugar_too_many_args = eval_script("'(a b c d)");
        // The quote prefix applies to one whole list, so this supplies one argument.
        assert!(sugar_too_many_args.is_ok());
    }
}
