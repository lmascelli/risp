//! `read`, `prin1-to-string` and `readablep`: round trips for every readable
//! type, exact written text, reader errors, refusals, depth and fuel.
#[cfg(test)]
mod tests {
    use crate::{
        Env, EvalError, LispContext, LispExp, Parser, ParserError, Unreadable, eval,
        setup_base_env, write_readable,
    };
    use std::collections::HashMap;
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    fn eval_str(source: &str) -> Result<LispExp<()>, EvalError<()>> {
        let env = Env::new_root();
        setup_base_env(env.clone());
        let wrapped = format!("(progn {source})");
        let ast = Parser::new(&wrapped)
            .next()
            .expect("failed to parse test script");
        eval(&ast, env, &())
    }

    fn eval_ok(source: &str) -> LispExp<()> {
        eval_str(source).unwrap_or_else(|why| panic!("eval of `{source}` failed: {why:?}"))
    }

    fn text(exp: &LispExp<()>) -> String {
        match exp {
            LispExp::String(text) => text.to_string(),
            other => panic!("expected a string, got {other:?}"),
        }
    }

    /// Write VALUE, read the text back as data, and require an equal value.
    fn round_trip(value: &LispExp<()>) -> String {
        let written = write_readable(value).unwrap_or_else(|why| panic!("{value:?}: {why:?}"));
        let read: LispExp<()> = Parser::for_data(&written)
            .next()
            .unwrap_or_else(|why| panic!("{written}: {why:?}"));
        assert_eq!(&read, value, "written as {written}");
        written
    }

    /// The condition symbol and data of a signal raised by SOURCE.
    fn signal_of(source: &str) -> (String, String) {
        match eval_str(source) {
            Err(EvalError::Signal { symbol, data }) => (format!("{symbol:?}"), format!("{data:?}")),
            other => panic!("expected `{source}` to signal, got {other:?}"),
        }
    }

    fn nested(depth: usize) -> LispExp<()> {
        let mut value = LispExp::number(1.0);
        for _ in 0..depth {
            value = LispExp::proper_list(vec![value]);
        }
        value
    }

    // Round trips

    #[test]
    fn lists_round_trip_as_data_including_dotted_and_quoted_ones() {
        for source in [
            "(1 2 3)",
            "(a (b (c)) d)",
            "(a . b)",
            "(1 2 . 3)",
            "((a . 1) (b . 2))",
            "(quote x)",
            "(backquote (a (unquote b) (unquote-splicing c)))",
            "(nil t)",
            "([1 2] {k (v)})",
        ] {
            let value: LispExp<()> = Parser::for_data(source).next().unwrap();
            assert_eq!(round_trip(&value), source);
        }
    }

    #[test]
    fn strings_round_trip_with_every_escape_the_reader_knows() {
        for raw in [
            "",
            "plain",
            "quote \" and backslash \\",
            "line\nfeed\ttab\rreturn",
            "escape \u{1b} and nul \0",
            "\\d+ stays a regex",
            "unicode é 漢 😀",
            "other control \u{7} kept as is",
        ] {
            round_trip(&LispExp::string(raw.into()));
        }
        assert_eq!(
            write_readable(&LispExp::<()>::string("a\"b\\c\nd".into())).unwrap(),
            r#""a\"b\\c\nd""#
        );
    }

    #[test]
    fn finite_numbers_round_trip_exactly() {
        for n in [
            0.0,
            -0.0,
            1.0,
            -42.0,
            0.1,
            0.1 + 0.2,
            1.0 / 3.0,
            -2.5e-7,
            1e21,
            1e300,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            5e-324,
            9_007_199_254_740_993.0,
        ] {
            let written = round_trip(&LispExp::number(n));
            assert!(!written.contains('e'), "{n} written as {written}");
        }
        assert_eq!(write_readable(&LispExp::<()>::number(3.0)).unwrap(), "3");
        assert_eq!(
            write_readable(&LispExp::<()>::number(-0.5)).unwrap(),
            "-0.5"
        );
    }

    #[test]
    fn symbols_that_read_back_as_themselves_round_trip() {
        for name in [
            "nil", "t", "foo", "foo-bar", "-", "1+", "1-", ":key", "a.b", "->", "*x*", "%", "1a",
            "a\"b", "#<x>", "a'b",
        ] {
            assert_eq!(round_trip(&LispExp::symbol(name.into())), name);
        }
    }

    #[test]
    fn vectors_and_maps_round_trip_with_sorted_and_quoted_keys() {
        let map = LispExp::<()>::map(HashMap::from([
            ("a b".into(), LispExp::number(1.0)),
            ("12".into(), LispExp::number(2.0)),
            ("".into(), LispExp::number(3.0)),
            (
                "plain".into(),
                LispExp::vec(vec![LispExp::symbol("x".into())]),
            ),
            ("nil".into(), LispExp::proper_list(vec![LispExp::nil()])),
        ]));
        assert_eq!(
            round_trip(&map),
            r#"{"" 3 "12" 2 "a b" 1 nil (nil) plain [x]}"#
        );
        let vector = LispExp::<()>::vec(vec![
            LispExp::vec(vec![]),
            LispExp::map(HashMap::new()),
            LispExp::string("s".into()),
            LispExp::improper_list(vec![LispExp::number(1.0)], LispExp::number(2.0)),
        ]);
        assert_eq!(round_trip(&vector), r#"[[] {} "s" (1 . 2)]"#);
    }

    #[test]
    fn generated_values_round_trip_or_are_refused_for_a_reason() {
        // A small xorshift keeps the cases reproducible without a dependency.
        struct Rng(u64);
        impl Rng {
            fn below(&mut self, bound: u64) -> u64 {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                self.0 % bound
            }
            fn word(&mut self) -> String {
                const CHARS: &[char] = &[
                    'a', 'z', '0', '9', '-', '.', ' ', '"', '\\', '\n', '\t', '\r', '(', ']', '{',
                    ';', '\'', '`', ',', '#', 'é', '😀', '\u{1b}', '\0',
                ];
                let len = self.below(5);
                (0..len)
                    .map(|_| CHARS[self.below(CHARS.len() as u64) as usize])
                    .collect()
            }
        }
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut written_cases = 0;

        for _ in 0..2_000 {
            let mut stack: Vec<LispExp<()>> = Vec::new();
            for _ in 0..(1 + rng.below(12)) {
                let value = match rng.below(9) {
                    0 => {
                        let n = rng.below(2_000_000) as f64 - 1e6;
                        LispExp::number(n / (1 + rng.below(1000)) as f64)
                    }
                    1 => LispExp::string(rng.word()),
                    2 => LispExp::symbol(rng.word()),
                    3 if !stack.is_empty() => {
                        let take = 1 + rng.below(stack.len() as u64) as usize;
                        LispExp::proper_list(stack.split_off(stack.len() - take))
                    }
                    4 if stack.len() >= 2 => {
                        let tail = stack.pop().unwrap();
                        let head = stack.pop().unwrap();
                        LispExp::cons(head, tail)
                    }
                    5 if !stack.is_empty() => {
                        let take = rng.below(stack.len() as u64 + 1) as usize;
                        LispExp::vec(stack.split_off(stack.len() - take))
                    }
                    6 if !stack.is_empty() => {
                        let take = rng.below(stack.len() as u64 + 1) as usize;
                        let values = stack.split_off(stack.len() - take);
                        LispExp::map(values.into_iter().map(|v| (rng.word(), v)).collect())
                    }
                    _ => LispExp::nil(),
                };
                stack.push(value);
            }
            let value = LispExp::vec(stack);

            match write_readable(&value) {
                Ok(written) => {
                    let read: LispExp<()> = Parser::for_data(&written)
                        .next()
                        .unwrap_or_else(|why| panic!("{written}: {why:?}"));
                    assert_eq!(read, value, "written as {written}");
                    written_cases += 1;
                }
                Err(Unreadable::Value(LispExp::Symbol(name))) => {
                    let read: Result<LispExp<()>, _> = Parser::for_data(&name).next();
                    assert_ne!(
                        read,
                        Ok(LispExp::Symbol(name.clone())),
                        "{name:?} was refused"
                    );
                }
                Err(other) => panic!("unexpected refusal {other:?} for {value:?}"),
            }
        }
        // Most cases must be written, or the round trip would test little.
        assert!(written_cases > 1_000, "only {written_cases} cases written");
    }

    // The Lisp primitives

    #[test]
    fn read_returns_data_without_evaluating_it() {
        assert_eq!(format!("{:?}", eval_ok(r#"(read "(+ 1 2)")"#)), "(+ 1 2)");
        assert!(eval_ok(r#"(consp (read "(+ 1 2)"))"#).is_truthy());
        assert!(eval_ok(r#"(consp (elt (read "[x (y)]") 1))"#).is_truthy());
        assert_eq!(format!("{:?}", eval_ok(r#"(read "'a")"#)), "(quote a)");
        assert_eq!(format!("{:?}", eval_ok(r#"(read " ; note\n 42 ")"#)), "42");
        assert_eq!(
            format!("{:?}", eval_ok(r#"(read "undefined-variable")"#)),
            "undefined-variable"
        );
    }

    #[test]
    fn read_of_prin1_to_string_gives_an_equal_value() {
        for source in [
            r#"'(a "b\n" 1.5 (c . d) [e {k "v"}])"#,
            r#""tab\there""#,
            "-0.125",
            "'sym",
            "{b [1 2] a '(x)}",
            "nil",
        ] {
            assert!(
                eval_ok(&format!(
                    "(let ((x {source})) (equal (read (prin1-to-string x)) x))"
                ))
                .is_truthy(),
                "{source}"
            );
        }
        assert_eq!(
            text(&eval_ok(r#"(prin1-to-string '(a "b" [1 2.5]))"#)),
            r#"(a "b" [1 2.5])"#
        );
    }

    #[test]
    fn read_signals_end_of_file_for_unfinished_text() {
        for source in [
            "",
            "   ",
            "; only a comment",
            "(1 2",
            "[1",
            "{a",
            "\"abc",
            "'",
        ] {
            let (symbol, _) = signal_of(&format!("(read {source:?})"));
            assert_eq!(symbol, "end-of-file", "{source:?}");
        }
    }

    #[test]
    fn read_signals_invalid_read_syntax_for_malformed_or_extra_text() {
        for (source, data) in [
            (")", r#"("UnbalancedRParen")"#),
            ("1.x", r#"("NumberParseError(\"1.x\")")"#),
            ("(a . )", r#"("UnexpectedToken(\".\")")"#),
            ("{1 2}", r#"("InvalidMapKey")"#),
            ("1 2", r#"("UnexpectedToken(\"a second value\")")"#),
            ("(a) b", r#"("UnexpectedToken(\"a second value\")")"#),
        ] {
            let (symbol, got) = signal_of(&format!("(read {source:?})"));
            assert_eq!(
                (symbol.as_str(), got.as_str()),
                ("invalid-read-syntax", data),
                "{source:?}"
            );
        }
        let deep = format!("{}{}", "(".repeat(257), ")".repeat(257));
        let (symbol, data) = signal_of(&format!("(read {deep:?})"));
        assert_eq!(
            (symbol.as_str(), data.as_str()),
            ("invalid-read-syntax", r#"("TooDeep")"#)
        );
    }

    #[test]
    fn read_errors_can_be_caught_by_condition_case() {
        assert_eq!(
            format!(
                "{:?}",
                eval_ok(r#"(condition-case nil (read "(") (end-of-file 'eof))"#)
            ),
            "eof"
        );
        assert_eq!(
            format!(
                "{:?}",
                eval_ok(r#"(condition-case nil (read ")") (error 'bad))"#)
            ),
            "bad"
        );
    }

    #[test]
    fn prin1_to_string_refuses_what_has_no_read_syntax() {
        for (source, part) in [
            ("(lambda (x) x)", "#<lambda (x)>"),
            ("(make-atom 1)", "#<atom>"),
            (r#"(string-to-number "inf")"#, "inf"),
            (r#"(string-to-number "NaN")"#, "NaN"),
            (r#"(intern "a b")"#, "a b"),
            (r#"(intern "")"#, ""),
            (r#"(intern "12")"#, "12"),
            (r#"(intern "-5")"#, "-5"),
            (r#"(intern ".")"#, "."),
            (r#"(intern "'a")"#, "'a"),
            (r#"(intern "a;b")"#, "a;b"),
            (r#"(intern "a(b")"#, "a(b"),
            (r#"(list 1 [(intern "x y")])"#, "x y"),
        ] {
            let (symbol, data) = signal_of(&format!("(prin1-to-string {source})"));
            assert_eq!(symbol, "wrong-type-argument", "{source}");
            assert!(data.starts_with("(readablep "), "{source}: {data}");
            let expected = format!("{:?}", eval_ok(source));
            if !source.starts_with("(list") {
                assert_eq!(data, format!("(readablep {expected})"), "{source}");
            }
            assert!(data.contains(part), "{source}: {data}");
            assert!(
                eval_ok(&format!("(null (readablep {source}))")).is_truthy(),
                "{source}"
            );
        }
    }

    #[test]
    fn primitives_and_closures_inside_data_are_refused() {
        let primitive = LispExp::<()>::primitive(|_, _, _| Ok(LispExp::nil()), None);
        let inside = LispExp::proper_list(vec![LispExp::number(1.0), primitive.clone()]);
        assert_eq!(write_readable(&inside), Err(Unreadable::Value(primitive)));
    }

    #[test]
    fn readablep_returns_the_written_text_or_nil() {
        assert_eq!(
            text(&eval_ok("(readablep '(1 \"a\" . b))")),
            r#"(1 "a" . b)"#
        );
        assert!(eval_ok("(null (readablep (list 1 (lambda () 2))))").is_truthy());
    }

    #[test]
    fn nesting_up_to_the_reader_limit_is_written_and_deeper_is_refused() {
        round_trip(&nested(256));
        assert_eq!(write_readable(&nested(257)), Err(Unreadable::TooDeep));

        let deep_vector = (0..257).fold(LispExp::<()>::nil(), |v, _| LispExp::vec(vec![v]));
        assert_eq!(write_readable(&deep_vector), Err(Unreadable::TooDeep));

        let env = Env::new_root();
        setup_base_env(env.clone());
        env.set_variable("deep".into(), nested(257));
        let ast = Parser::new("(prin1-to-string deep)").next().unwrap();
        match eval(&ast, env, &()) {
            Err(EvalError::Signal { symbol, data }) => {
                assert_eq!(format!("{symbol:?}"), "wrong-type-argument");
                assert_eq!(
                    data,
                    LispExp::proper_list(vec![LispExp::symbol("readablep".into()), nested(257)])
                );
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn long_lists_are_written_without_deep_recursion() {
        let long =
            LispExp::<()>::proper_list((0..200_000).map(|i| LispExp::number(i as f64)).collect());
        let written = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || write_readable(&long).map(|text| text.len()))
            .unwrap()
            .join()
            .unwrap();
        assert!(written.unwrap() > 200_000);
    }

    #[test]
    fn the_data_reader_never_produces_syntax_forms() {
        let value: LispExp<()> = Parser::for_data("(a [b (c)] {k (d . (e f))} 'g)")
            .next()
            .unwrap();
        fn has_form(exp: &LispExp<()>) -> bool {
            match exp {
                LispExp::Form(_) => true,
                LispExp::Cons(cell) => has_form(&cell.car) || has_form(&cell.cdr),
                LispExp::Vector(items) => items.iter().any(has_form),
                LispExp::Map(map) => map.values().any(has_form),
                _ => false,
            }
        }
        assert!(!has_form(&value), "{value:?}");
        assert!(matches!(
            Parser::new("(a)").next::<()>(),
            Ok(LispExp::Form(_))
        ));
    }

    #[test]
    fn every_new_docstring_starts_with_its_signature() {
        for name in ["read", "prin1-to-string", "readablep"] {
            let doc = text(&eval_ok(&format!("(function-doc '{name})")));
            assert!(
                doc.starts_with(&format!("({name} ")) && doc.contains("): "),
                "{doc}"
            );
        }
    }

    // Fuel

    #[derive(Clone, Debug)]
    struct Budget(Arc<AtomicU32>);

    impl PartialEq for Budget {
        fn eq(&self, other: &Self) -> bool {
            Arc::ptr_eq(&self.0, &other.0)
        }
    }

    impl LispContext for Budget {
        fn consume_fuel(&self, amount: u32) -> Result<(), EvalError<Self>> {
            self.0
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                    left.checked_sub(amount)
                })
                .map(|_| ())
                .map_err(|_| EvalError::OutOfFuel)
        }
    }

    #[test]
    fn reading_and_writing_charge_one_unit_per_byte() {
        let env = Env::new_root();
        setup_base_env(env.clone());
        let big = format!("\"{}\"", "x".repeat(5_000));
        env.set_variable("big".into(), LispExp::string("x".repeat(5_000)));
        env.set_variable("big-text".into(), LispExp::string(big));
        for source in [
            "(read big-text)",
            "(prin1-to-string big)",
            "(readablep big)",
        ] {
            let ast = Parser::new(source).next().unwrap();
            let small = Budget(Arc::new(AtomicU32::new(1_000)));
            assert!(
                matches!(eval(&ast, env.clone(), &small), Err(EvalError::OutOfFuel)),
                "{source}"
            );
            let large = Budget(Arc::new(AtomicU32::new(100_000)));
            assert!(eval(&ast, env.clone(), &large).is_ok(), "{source}");
            assert!(
                100_000 - large.0.load(Ordering::Relaxed) >= 5_000,
                "{source}"
            );
        }
    }

    #[test]
    fn a_failed_data_read_reports_the_reader_error() {
        assert_eq!(
            Parser::for_data("(a").next::<()>(),
            Err(ParserError::UnclosedList)
        );
    }
}
