//! JSON: parsing every value type, error positions, writing, refusals, round
//! trips, deep nesting, fuel, and an explicit timing run on a 1 MB document.
#[cfg(test)]
mod tests {
    use crate::{
        Env, EvalError, JsonError, LispContext, LispExp, Parser, eval, parse_json, setup_base_env,
        write_json,
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

    fn parse(text: &str) -> LispExp<()> {
        parse_json(text).unwrap_or_else(|why| panic!("{text}: {why:?}"))
    }

    fn sym(name: &str) -> LispExp<()> {
        LispExp::symbol(name.into())
    }

    fn json(value: &LispExp<()>) -> String {
        write_json(value).unwrap_or_else(|part| panic!("{value:?} refused at {part:?}"))
    }

    // Parsing

    #[test]
    fn every_json_type_becomes_the_agreed_lisp_value() {
        assert_eq!(parse("true"), LispExp::t());
        assert_eq!(parse("false"), sym("false"));
        assert_eq!(parse("null"), sym("null"));
        assert!(!parse("false").is_nil() && !parse("null").is_nil());
        assert_eq!(parse("\"hi\""), LispExp::string("hi".into()));
        assert_eq!(parse("[]"), LispExp::vec(vec![]));
        assert_eq!(parse("{}"), LispExp::map(HashMap::new()));
        assert_eq!(
            parse(r#" { "a" : [ 1 , -2.5e3 , true ] , "b" : { } } "#),
            LispExp::map(HashMap::from([
                (
                    "a".into(),
                    LispExp::vec(vec![
                        LispExp::number(1.0),
                        LispExp::number(-2500.0),
                        LispExp::t()
                    ])
                ),
                ("b".into(), LispExp::map(HashMap::new())),
            ]))
        );
    }

    #[test]
    fn numbers_follow_the_json_grammar() {
        for (text, n) in [
            ("0", 0.0),
            ("-0", -0.0),
            ("12", 12.0),
            ("-3.25", -3.25),
            ("1e3", 1000.0),
            ("1E+3", 1000.0),
            ("2.5e-3", 0.0025),
            ("9007199254740993", 9007199254740992.0),
            ("1e308", 1e308),
        ] {
            assert_eq!(parse(text), LispExp::number(n), "{text}");
        }
    }

    #[test]
    fn string_escapes_decode_including_surrogate_pairs() {
        assert_eq!(
            parse(r#""q\" b\\ s\/ \b\f\n\r\t Aé漢 😀""#),
            LispExp::string("q\" b\\ s/ \u{8}\u{c}\n\r\t Aé漢 😀".into())
        );
        assert_eq!(parse("\"é😀 raw\""), LispExp::string("é😀 raw".into()));
    }

    #[test]
    fn a_repeated_key_keeps_its_last_value() {
        assert_eq!(
            parse(r#"{"k": 1, "k": 2}"#),
            LispExp::map(HashMap::from([("k".into(), LispExp::number(2.0))]))
        );
    }

    #[test]
    fn errors_report_line_column_and_character_position() {
        for (text, message, line, column, position) in [
            ("", "unexpected end of input, expected a value", 1, 1, 0),
            ("   ", "unexpected end of input, expected a value", 1, 4, 3),
            ("[1,]", "unexpected ']', expected a value", 1, 4, 3),
            ("[1 2]", "unexpected '2', expected ',' or ']'", 1, 4, 3),
            (r#"{"a" 1}"#, "unexpected '1', expected ':'", 1, 6, 5),
            ("{1: 2}", "unexpected '1', expected a string key", 1, 2, 1),
            (
                r#"{"a": 1,}"#,
                "unexpected '}', expected a string key",
                1,
                9,
                8,
            ),
            (
                "[1",
                "unexpected end of input, expected ',' or ']'",
                1,
                3,
                2,
            ),
            ("\"abc", "unexpected end of input in string", 1, 5, 4),
            ("tru", "invalid literal, expected true", 1, 1, 0),
            ("nul ", "invalid literal, expected null", 1, 1, 0),
            ("01", "unexpected '1', expected the end of input", 1, 2, 1),
            ("1 2", "unexpected '2', expected the end of input", 1, 3, 2),
            ("-", "unexpected end of input, expected a digit", 1, 2, 1),
            (
                "1.",
                "unexpected end of input, expected a digit after '.'",
                1,
                3,
                2,
            ),
            (
                "1e",
                "unexpected end of input, expected a digit in the exponent",
                1,
                3,
                2,
            ),
            ("+1", "unexpected '+', expected a value", 1, 1, 0),
            ("1e400", "number out of range", 1, 1, 0),
            ("\"a\u{1}\"", "control character in string", 1, 3, 2),
            (
                r#""\x""#,
                "unexpected 'x', expected an escape such as \\n or \\u",
                1,
                3,
                2,
            ),
            (r#""\u12g4""#, "expected four hex digits after \\u", 1, 4, 3),
            (r#""\ud800""#, "unpaired surrogate in \\u escape", 1, 2, 1),
            (r#""\ud800A""#, "unpaired surrogate in \\u escape", 1, 2, 1),
            (r#""\udc00""#, "unpaired surrogate in \\u escape", 1, 2, 1),
            ("[\n  1,\n  x]", "unexpected 'x', expected a value", 3, 3, 9),
            ("[\"é😀\", x]", "unexpected 'x', expected a value", 1, 8, 7),
            ("NaN", "unexpected 'N', expected a value", 1, 1, 0),
        ] {
            assert_eq!(
                parse_json::<()>(text),
                Err(JsonError {
                    message: message.into(),
                    line,
                    column,
                    position
                }),
                "{text:?}"
            );
        }
    }

    // Writing

    #[test]
    fn values_are_written_as_compact_json_with_sorted_keys() {
        let value = LispExp::map(HashMap::from([
            (
                "b".into(),
                LispExp::vec(vec![LispExp::number(1.0), LispExp::number(2.5)]),
            ),
            ("a".into(), LispExp::string("x".into())),
            (
                "c".into(),
                LispExp::vec(vec![LispExp::t(), sym("false"), sym("null")]),
            ),
            ("d".into(), LispExp::map(HashMap::new())),
        ]));
        assert_eq!(
            json(&value),
            r#"{"a":"x","b":[1,2.5],"c":[true,false,null],"d":{}}"#
        );
    }

    #[test]
    fn strings_escape_quotes_backslashes_and_control_characters() {
        assert_eq!(
            json(&LispExp::string(
                "q\" b\\ \n\r\t\u{8}\u{c}\u{1}\u{1f} é😀 /".into()
            )),
            r#""q\" b\\ \n\r\t\b\f\u0001\u001f é😀 /""#
        );
    }

    #[test]
    fn numbers_are_written_briefly_and_read_back_exactly() {
        for (n, text) in [
            (3.0, "3"),
            (-0.5, "-0.5"),
            (0.1, "0.1"),
            (1e21, "1e21"),
            (1e300, "1e300"),
            (5e-324, "5e-324"),
            (-9007199254740991.0, "-9007199254740991"),
        ] {
            assert_eq!(json(&LispExp::number(n)), text);
            assert_eq!(parse(text), LispExp::number(n));
        }
        for n in [
            1.0 / 3.0,
            f64::MAX,
            f64::MIN_POSITIVE,
            123456.789e10,
            0.1 + 0.2,
        ] {
            assert_eq!(parse(&json(&LispExp::number(n))), LispExp::number(n), "{n}");
        }
    }

    #[test]
    fn values_without_json_meaning_are_refused_with_the_offending_part() {
        let lambda = eval_ok("(lambda (x) x)");
        for (value, part) in [
            (LispExp::nil(), LispExp::nil()),
            (sym("foo"), sym("foo")),
            (
                LispExp::proper_list(vec![LispExp::number(1.0)]),
                LispExp::proper_list(vec![LispExp::number(1.0)]),
            ),
            (
                LispExp::number(f64::INFINITY),
                LispExp::number(f64::INFINITY),
            ),
            (
                LispExp::vec(vec![LispExp::number(1.0), LispExp::nil()]),
                LispExp::nil(),
            ),
            (
                LispExp::map(HashMap::from([("f".into(), lambda.clone())])),
                lambda.clone(),
            ),
        ] {
            assert_eq!(write_json(&value), Err(part), "{value:?}");
        }
    }

    #[test]
    fn writing_then_parsing_gives_back_the_value() {
        let value = parse(
            r#"{"jsonrpc":"2.0","id":7,"params":{"uri":"file:///a b/é.rs",
                "items":[{"x":-1.5e-9,"ok":true},{"x":0,"ok":false},null],"empty":[]}}"#,
        );
        assert_eq!(parse(&json(&value)), value);
        let text = json(&value);
        assert_eq!(json(&parse(&text)), text);
    }

    #[test]
    fn deep_nesting_needs_no_deep_recursion() {
        let depth = 100_000;
        let text = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let ok = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let value: LispExp<()> = parse_json(&text).expect("deep array parses");
                write_json(&value)
                    .map(|written| written == text)
                    .unwrap_or(false)
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(ok);
    }

    // The Lisp primitives

    #[test]
    fn json_parse_string_and_json_serialize_work_from_lisp() {
        assert_eq!(
            format!(
                "{:?}",
                eval_ok(r#"(json-parse-string "{\"a\": [1, true, null]}")"#)
            ),
            "{a [1 t null]}"
        );
        assert_eq!(
            format!("{:?}", eval_ok(r#"(json-parse-string "false")"#)),
            "false"
        );
        assert_eq!(
            eval_ok(r#"(json-serialize {b [1 2.5] a "x"})"#),
            LispExp::string(r#"{"a":"x","b":[1,2.5]}"#.into())
        );
        assert_eq!(
            eval_ok("(json-serialize '[t false null])"),
            LispExp::string("[true,false,null]".into())
        );
        assert!(
            eval_ok(
                r#"(let ((v (json-parse-string "{\"k\": [\"s\", 1.5, {}]}")))
                     (equal (json-parse-string (json-serialize v)) v))"#
            )
            .is_truthy()
        );
        assert_eq!(
            format!(
                "{:?}",
                eval_ok(r#"(map-elt (json-parse-string "{\"k\": null}") "k")"#)
            ),
            "null"
        );
    }

    #[test]
    fn a_parse_error_signals_json_parse_error_with_its_position() {
        match eval_str(r#"(json-parse-string "[1,\n x]")"#) {
            Err(EvalError::Signal { symbol, data }) => {
                assert_eq!(symbol, sym("json-parse-error"));
                assert_eq!(
                    format!("{data:?}"),
                    r#"("unexpected 'x', expected a value" 2 2 5)"#
                );
            }
            other => panic!("expected a signal, got {other:?}"),
        }
        assert_eq!(
            format!(
                "{:?}",
                eval_ok(
                    r#"(condition-case e (json-parse-string "{") (json-parse-error (nth 4 e)))"#
                )
            ),
            "1"
        );
    }

    #[test]
    fn json_serialize_signals_wrong_type_argument_for_the_offending_part() {
        match eval_str("(json-serialize [1 nil])") {
            Err(EvalError::Signal { symbol, data }) => {
                assert_eq!(symbol, sym("wrong-type-argument"));
                assert_eq!(format!("{data:?}"), "(json-value-p nil)");
            }
            other => panic!("expected a signal, got {other:?}"),
        }
        assert!(
            eval_ok("(condition-case nil (json-serialize '(1 2)) (wrong-type-argument 'refused))")
                == sym("refused")
        );
    }

    #[test]
    fn every_new_docstring_starts_with_its_signature() {
        for name in ["json-parse-string", "json-serialize"] {
            let LispExp::String(doc) = eval_ok(&format!("(function-doc '{name})")) else {
                panic!("{name} has no documentation");
            };
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
    fn parsing_and_writing_charge_one_unit_per_byte() {
        let env = Env::new_root();
        setup_base_env(env.clone());
        let text = format!("[{}0]", "0,".repeat(2_500));
        env.set_variable("text".into(), LispExp::string(text.clone()));
        env.set_variable("value".into(), parse_json(&text).unwrap());
        for source in ["(json-parse-string text)", "(json-serialize value)"] {
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

    // Timing

    /// A document shaped like a batch of editor diagnostics, at least 1 MiB.
    fn one_megabyte_document() -> String {
        let mut items = Vec::new();
        let mut size = 0;
        let mut i = 0usize;
        while size < 1 << 20 {
            let item = format!(
                concat!(
                    r#"{{"range":{{"start":{{"line":{},"character":{}}},"#,
                    r#""end":{{"line":{},"character":{}}}}},"severity":{},"#,
                    r#""code":"E{:04}","source":"rustc","tags":[1,2],"deprecated":{},"#,
                    r#""data":null,"message":"mismatched types: expected \"u32\", found \"&str\" — é {}","#,
                    r#""score":{}}}"#
                ),
                i,
                i % 120,
                i + 1,
                (i * 7) % 120,
                1 + i % 4,
                i % 1000,
                if i.is_multiple_of(2) { "true" } else { "false" },
                i,
                (i as f64) / 7.0,
            );
            size += item.len() + 1;
            items.push(item);
            i += 1;
        }
        format!("{{\"jsonrpc\":\"2.0\",\"items\":[{}]}}", items.join(","))
    }

    #[test]
    #[ignore = "run explicitly: cargo test --release json_timing -- --ignored --nocapture"]
    fn json_timing_on_a_one_megabyte_document() {
        use std::time::{Duration, Instant};

        let text = one_megabyte_document();
        let median = |mut samples: Vec<Duration>| {
            samples.sort();
            samples[samples.len() / 2]
        };

        // Each parsed value is dropped after its sample is taken, so freeing
        // the previous document is not counted as parsing time.
        let mut parse_times = Vec::new();
        let mut drop_times = Vec::new();
        for _ in 0..9 {
            let start = Instant::now();
            let parsed = parse_json::<()>(&text).expect("the document parses");
            parse_times.push(start.elapsed());
            let start = Instant::now();
            drop(parsed);
            drop_times.push(start.elapsed());
        }
        let value = parse_json::<()>(&text).expect("the document parses");

        let mut write_times = Vec::new();
        for _ in 0..9 {
            let start = Instant::now();
            let written = write_json(&value).expect("the document writes");
            write_times.push(start.elapsed());
            drop(written);
        }
        let written = write_json(&value).expect("the document writes");

        assert_eq!(parse_json::<()>(&written).unwrap(), value);
        let mib = text.len() as f64 / (1 << 20) as f64;
        let parse = median(parse_times);
        let write = median(write_times);
        let freeing = median(drop_times);
        println!(
            "json: {} bytes ({mib:.2} MiB); parse median {parse:?} ({:.0} MiB/s); \
             serialize median {write:?} ({:.0} MiB/s); drop median {freeing:?}; \
             9 samples each",
            text.len(),
            mib / parse.as_secs_f64(),
            mib / write.as_secs_f64(),
        );
    }
}
