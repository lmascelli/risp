#[cfg(test)]
mod tests {
    use crate::{
        EvalError, LispContext, LispExp, Parser, Risp, exact_arity,
    };

    #[derive(Clone, Debug, PartialEq)]
    struct Host {
        answer: f64,
    }

    impl LispContext for Host {}

    #[test]
    fn environment_persists_between_evaluations() {
        let mut risp = Risp::new(Host { answer: 42.0 });

        let definition = Parser::new("(setq answer 42)")
            .next::<Host>()
            .unwrap();

        risp.eval_form(&definition).unwrap();

        let lookup = Parser::new("answer")
            .next::<Host>()
            .unwrap();

        assert_eq!(
            risp.eval_form(&lookup).unwrap(),
            LispExp::number(42.0),
        );
    }

    #[test]
    fn registered_primitive_receives_the_host() {
        let mut risp = Risp::new(Host { answer: 42.0 });

        risp.register_function("host-answer", |args, _env, host| {
            exact_arity(args, 0)?;
            Ok(LispExp::number(host.answer))
        });

        assert_eq!(
            risp.call("host-answer", &[]).unwrap(),
            LispExp::number(42.0),
        );
    }

    #[test]
    fn call_passes_symbols_as_data() {
        let mut risp = Risp::new(Host { answer: 42.0 });
        let value = LispExp::symbol("unbound-name".to_owned());

        let result = risp
            .call("list", &[value.clone()])
            .unwrap();

        assert_eq!(result, LispExp::proper_list(vec![value]));
    }

    #[test]
    fn missing_function_returns_an_error() {
        let mut risp = Risp::new(Host { answer: 42.0 });

        assert_eq!(
            risp.call("missing-function", &[]),
            Err(EvalError::UndefinedFunction(
                "missing-function".to_owned(),
            )),
        );
    }
}