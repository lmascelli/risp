#[cfg(test)]
mod tests {
    use crate::{Env, LispExp, bootstrap_vm, setup_base_env};
    use crate::{EvalError, LispContext, Parser, eval};
    use std::sync::RwLock;

    #[derive(Debug)]
    struct MockHostContext {
        pub fuel_remaining: RwLock<u32>,
        pub logs: RwLock<Vec<String>>,
    }

    impl Clone for MockHostContext {
        fn clone(&self) -> Self {
            unreachable!()
        }
    }

    impl PartialEq for MockHostContext {
        fn eq(&self, _other: &Self) -> bool {
            unreachable!()
        }
    }

    impl LispContext for MockHostContext {
        fn consume_fuel(&self, amount: u32) -> Result<(), EvalError<MockHostContext>> {
            if *self.fuel_remaining.read().unwrap() < amount {
                Err(EvalError::OutOfFuel)
            } else {
                *self.fuel_remaining.write().unwrap() -= amount;
                Ok(())
            }
        }

        fn log_diagnostic(&self, msg: &str) {
            self.logs.write().unwrap().push(msg.to_string());
        }
    }

    #[test]
    fn test_successful_bootstrap_handshake() {
        let mut ctx = MockHostContext {
            fuel_remaining: RwLock::new(5000),
            logs: RwLock::new(vec![]),
        };

        let env_res = bootstrap_vm(&mut ctx);
        assert!(env_res.is_ok());
        assert!(
            ctx.logs.read().unwrap().contains(
                &"VM Handshake: State verification successful. Core is stable.".to_string()
            )
        );
    }

    #[test]
    fn test_fuel_system_stops_infinite_loops() {
        let mut ctx = MockHostContext {
            fuel_remaining: 100.into(),
            logs: vec![].into(),
        };

        let env = bootstrap_vm(&mut ctx).unwrap();

        let malicious_script = "(while t (setq x 1))";
        let mut parser = Parser::new(malicious_script);
        let ast = parser.next().unwrap();

        let res = eval(&ast, env, &mut ctx);

        assert_eq!(res, Err(EvalError::OutOfFuel));
        assert_eq!(*ctx.fuel_remaining.read().unwrap(), 0);
    }

    fn quote(name: &str) -> LispExp<MockHostContext> {
        LispExp::form(vec![
            LispExp::symbol("quote".into()),
            LispExp::symbol(name.into()),
        ])
    }

    #[test]
    fn test_add_to_list_prepends_and_ignores_duplicates() {
        let env = Env::new_root();
        setup_base_env(env.clone());

        let mut ctx = MockHostContext {
            fuel_remaining: RwLock::new(5000),
            logs: RwLock::new(vec![]),
        };

        let initial_list = LispExp::proper_list(vec![LispExp::number(2.0), LispExp::number(3.0)]);
        env.set_variable("my-list".into(), initial_list);

        let exp_add_1 = LispExp::form(vec![
            LispExp::symbol("add-to-list".into()),
            quote("my-list"),
            LispExp::number(1.0),
        ]);
        eval(&exp_add_1, env.clone(), &mut ctx).unwrap();

        let current_list = env.get_variable("my-list").unwrap();
        assert_eq!(
            current_list,
            LispExp::proper_list(vec![
                LispExp::number(1.0),
                LispExp::number(2.0),
                LispExp::number(3.0)
            ])
        );

        let exp_add_duplicate = LispExp::form(vec![
            LispExp::symbol("add-to-list".into()),
            quote("my-list"),
            LispExp::number(2.0),
        ]);
        eval(&exp_add_duplicate, env.clone(), &mut ctx).unwrap();

        let unmodified_list = env.get_variable("my-list").unwrap();
        assert_eq!(
            unmodified_list,
            LispExp::proper_list(vec![
                LispExp::number(1.0),
                LispExp::number(2.0),
                LispExp::number(3.0)
            ])
        );
    }

    #[test]
    fn test_append_to_list_appends_and_ignores_duplicates() {
        let env = Env::new_root();
        setup_base_env(env.clone());
        let mut ctx = MockHostContext {
            fuel_remaining: RwLock::new(5000),
            logs: RwLock::new(vec![]),
        };

        let initial_list = LispExp::proper_list(vec![LispExp::number(1.0), LispExp::number(2.0)]);
        env.set_variable("my-list".into(), initial_list);

        let exp_append_3 = LispExp::form(vec![
            LispExp::symbol("append-to-list".into()),
            quote("my-list"),
            LispExp::number(3.0),
        ]);
        eval(&exp_append_3, env.clone(), &mut ctx).unwrap();

        let current_list = env.get_variable("my-list").unwrap();
        assert_eq!(
            current_list,
            LispExp::proper_list(vec![
                LispExp::number(1.0),
                LispExp::number(2.0),
                LispExp::number(3.0)
            ])
        );

        let exp_append_duplicate = LispExp::form(vec![
            LispExp::symbol("append-to-list".into()),
            quote("my-list"),
            LispExp::number(2.0),
        ]);
        eval(&exp_append_duplicate, env.clone(), &mut ctx).unwrap();

        let unmodified_list = env.get_variable("my-list").unwrap();
        assert_eq!(
            unmodified_list,
            LispExp::proper_list(vec![
                LispExp::number(1.0),
                LispExp::number(2.0),
                LispExp::number(3.0)
            ])
        );
    }

    #[test]
    fn test_remove_from_list_handles_multiple_removals() {
        let env = Env::new_root();
        setup_base_env(env.clone());
        let mut ctx = MockHostContext {
            fuel_remaining: RwLock::new(5000),
            logs: RwLock::new(vec![]),
        };

        let initial_list = LispExp::proper_list(vec![
            LispExp::number(1.0),
            LispExp::number(2.0),
            LispExp::number(3.0),
            LispExp::number(4.0),
            LispExp::number(5.0),
        ]);
        env.set_variable("my-list".into(), initial_list);

        let exp_remove = LispExp::form(vec![
            LispExp::symbol("remove-from-list".into()),
            quote("my-list"),
            LispExp::number(2.0),
            LispExp::number(4.0),
        ]);
        eval(&exp_remove, env.clone(), &mut ctx).unwrap();

        let current_list = env.get_variable("my-list").unwrap();
        assert_eq!(
            current_list,
            LispExp::proper_list(vec![
                LispExp::number(1.0),
                LispExp::number(3.0),
                LispExp::number(5.0)
            ])
        );

        let exp_remove_missing = LispExp::form(vec![
            LispExp::symbol("remove-from-list".into()),
            quote("my-list"),
            LispExp::number(99.0),
        ]);
        eval(&exp_remove_missing, env.clone(), &mut ctx).unwrap();

        let unmodified_list = env.get_variable("my-list").unwrap();
        assert_eq!(
            unmodified_list,
            LispExp::proper_list(vec![
                LispExp::number(1.0),
                LispExp::number(3.0),
                LispExp::number(5.0)
            ])
        );
    }
}
