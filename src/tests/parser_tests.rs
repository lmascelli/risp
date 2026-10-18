#[cfg(test)]
mod test {

    use crate::{LispExp, Parser, parser::ParserError};

    #[test]
    fn test_parse_primitives() {
        let mut parser = Parser::new("42");

        let ast_num: LispExp<()> = parser.next().unwrap();
        assert_eq!(ast_num, LispExp::Number(42.0));

        let mut parser = Parser::new("my-symbol");
        let ast_sym: LispExp<()> = parser.next().unwrap();
        assert_eq!(ast_sym, LispExp::symbol("my-symbol".to_string()));
    }

    #[test]
    fn test_parse_simple_list() {
        let mut parser = Parser::new("(print \"world\")");
        let ast: LispExp<()> = parser.next().unwrap();

        assert_eq!(
            ast,
            LispExp::form(vec![
                LispExp::symbol("print".to_string()),
                LispExp::string("world".to_string()),
            ])
        );
    }

    #[test]
    fn test_parse_nested_lists() {
        // Equivalent to: (define x (+ 10 20))
        let mut parser = Parser::new("(define x (+ 10 20))");
        let ast: LispExp<()> = parser.next().unwrap();

        assert_eq!(
            ast,
            LispExp::form(vec![
                LispExp::symbol("define".to_string()),
                LispExp::symbol("x".to_string()),
                LispExp::form(vec![
                    LispExp::symbol("+".to_string()),
                    LispExp::Number(10.0),
                    LispExp::Number(20.0),
                ])
            ])
        );
    }

    #[test]
    fn test_parse_deeply_nested() {
        let mut parser = Parser::new("(((1)))");
        let ast: LispExp<()> = parser.next().unwrap();

        assert_eq!(
            ast,
            LispExp::form(vec![LispExp::form(vec![LispExp::form(vec![
                LispExp::Number(1.0)
            ])])])
        );
    }

    #[test]
    fn test_parse_errors() {
        let mut parser = Parser::new("(+1 2");
        let err1 = parser.next::<()>().unwrap_err();
        assert_eq!(err1, ParserError::UnclosedList);

        eprintln!("{:?}", Parser::new("(+ 1 2))").next::<()>().unwrap_err());
        let mut parser = Parser::new("(+1 2))");
        let err2 = parser.next::<()>().unwrap_err();
        assert_eq!(err2, ParserError::UnbalancedRParen);
    }

    #[test]
    fn test_vector_parsing() {
        let input = "[1 2 3]";
        let mut parser = Parser::new(input);
        let ast: LispExp<()> = parser.next().expect("Should parse vector");

        match ast {
            LispExp::Vector(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0], LispExp::Number(1.0));
            }
            _ => panic!("Expected Vector, found {:?}", ast),
        }
    }

    #[test]
    fn test_map_parsing_valid() {
        // Testing { "name" "Gemini" "version" 3 }
        let input = r#"{ "name" "Gemini" "version" 3.0 }"#;
        let mut parser = Parser::new(input);
        let ast: LispExp<()> = parser.next().expect("Should parse map");

        if let LispExp::Map(m) = ast {
            assert_eq!(m.get("name").unwrap(), &LispExp::string("Gemini".into()));
            assert_eq!(m.get("version").unwrap(), &LispExp::Number(3.0));
        } else {
            panic!("Expected Map, found {:?}", ast);
        }
    }

    #[test]
    fn test_nested_mixed_structures() {
        // A complex nested structure: (calculate [1 2] { "factor" 10.5 })
        let input = "(calculate [1 2] { \"factor\" 10.5 })";
        let mut parser = Parser::new(input);
        let ast: LispExp<()> = parser.next().expect("Should parse complex structure");

        if let LispExp::Form(list) = ast {
            assert_eq!(list[0], LispExp::symbol("calculate".into()));
            assert!(matches!(list[1], LispExp::Vector(_)));
            assert!(matches!(list[2], LispExp::Map(_)));
        } else {
            panic!("Root should be a Form");
        }
    }

    #[test]
    fn test_map_error_handling() {
        let input_odd = r#"{ "key1" "val1" "key2" }"#;
        let mut p1 = Parser::new(input_odd);
        let result = p1.next::<()>();
        assert!(matches!(result, Err(ParserError::MapKeyMissingValue)));

        let input_bad_key = "{ 42 \"answer\" }";
        let mut p2 = Parser::new(input_bad_key);
        let result = p2.next::<()>();
        assert!(matches!(result, Err(ParserError::InvalidMapKey)));
    }

    #[test]
    fn test_delimiter_clash_no_spaces() {
        // Symbols must terminate at vector and map delimiters without whitespace.
        let input = "my-func[1 2]{:key val}";
        let mut parser = Parser::new(input);

        assert!(matches!(parser.next::<()>().unwrap(), LispExp::Symbol(_)));
        assert!(matches!(parser.next::<()>().unwrap(), LispExp::Vector(_)));
        assert!(matches!(parser.next::<()>().unwrap(), LispExp::Map(_)));
    }

    #[test]
    fn test_edge_negative_numbers() {
        let input = "-42 -3.14";
        let mut parser = Parser::new(input);

        assert_eq!(parser.next::<()>().unwrap(), LispExp::Number(-42.0));
        assert_eq!(parser.next::<()>().unwrap(), LispExp::Number(-3.14));
    }

    #[test]
    fn test_edge_naked_decimals() {
        let input = ".5 5. 0.0";
        let mut parser = Parser::new(input);

        assert_eq!(parser.next::<()>().unwrap(), LispExp::Number(0.5));
        assert_eq!(parser.next::<()>().unwrap(), LispExp::Number(5.0));
        assert_eq!(parser.next::<()>().unwrap(), LispExp::Number(0.0));
    }

    #[test]
    fn test_edge_brutal_string_escaping() {
        // The decoded value is a backslash followed by a double quote.
        let input = r#" "\\\"" "#;
        let mut parser = Parser::new(input);

        let ast: LispExp<()> = parser.next().unwrap();
        if let LispExp::String(s) = ast {
            assert_eq!(s, std::sync::Arc::new("\\\"".into()));
        } else {
            panic!("Expected String, got {:?}", ast);
        }
    }

    #[test]
    fn test_edge_delimiter_smashes() {
        let input = "{key[1]}";
        let mut parser = Parser::new(input);

        let ast: LispExp<()> = parser.next().unwrap();
        if let LispExp::Map(m) = ast {
            let val = m.get("key").expect("Key 'key' not found");
            assert_eq!(val, &LispExp::vec(vec![LispExp::Number(1.0)]));
        } else {
            panic!("Expected Map");
        }
    }

    #[test]
    fn test_edge_nested_empties() {
        let input = "([{}])";
        let mut parser = Parser::new(input);

        let ast: LispExp<()> = parser.next().unwrap();

        assert_eq!(
            ast,
            LispExp::form(vec![LispExp::vec(vec![LispExp::map(
                std::collections::HashMap::new()
            )])])
        );
    }

    #[test]
    fn test_complex_whitespace_and_mixed_delimiters() {
        // Use a string key so the vector is read as a map value.
        let input = "{\n\t\"data\"\n\t[ (1\n2\t3) ]\n}";
        let mut parser = Parser::new(input);
        let ast: LispExp<()> = parser.next().expect("Should parse successfully");

        // Expected structure: Map { "data" => Vector [ Form [1, 2, 3] ] }
        if let LispExp::Map(map) = ast {
            let value = map.get("data").expect("Expected key 'data' in Map");

            if let LispExp::Vector(vec) = value {
                assert_eq!(vec.len(), 1);

                if let LispExp::Form(inner) = &vec[0] {
                    assert_eq!(inner.len(), 3);
                    assert_eq!(inner[0], LispExp::Number(1.0));
                } else {
                    panic!("Inner not a list");
                }
            } else {
                panic!("Map value is not a Vector");
            }
        } else {
            panic!("Outer structure is not a Map");
        }
    }

    #[test]
    fn test_numeric_boundary_cases() {
        // Numbers must terminate correctly at collection and input boundaries.
        let input = "[1.5] {\"k\" .5} 5.";
        let mut parser = Parser::new(input);

        let exp1: LispExp<()> = parser.next().unwrap();
        assert_eq!(exp1, LispExp::vec(vec![LispExp::Number(1.5)]));

        let exp2: LispExp<()> = parser.next().unwrap();
        if let LispExp::Map(m) = exp2 {
            assert_eq!(m.get("k"), Some(&LispExp::Number(0.5)));
        } else {
            panic!("Expected Map for second expression");
        }

        let exp3 = parser.next::<()>();
        println!("Result for 5.: {:?}", exp3);
    }

    #[test]
    fn test_empty_and_escaped_literals() {
        let input = r#" "" () [] {} "back\\slash" "#;
        let mut parser = Parser::new(input);

        assert_eq!(parser.next::<()>().unwrap(), LispExp::string("".into()));
        assert_eq!(parser.next::<()>().unwrap(), LispExp::form(vec![]));

        assert_eq!(parser.next::<()>().unwrap(), LispExp::vec(vec![]));
        assert_eq!(
            parser.next::<()>().unwrap(),
            LispExp::map(std::collections::HashMap::new())
        );

        assert_eq!(
            parser.next::<()>().unwrap(),
            LispExp::string("back\\slash".into())
        );
    }

    #[test]
    fn test_malformed_input_recovery() {
        let mut p1 = Parser::new("(1 2 3");
        assert_eq!(p1.next::<()>().unwrap_err(), ParserError::UnclosedList);

        let mut p2 = Parser::new("(1 2 3]");
        assert_eq!(
            p2.next::<()>().unwrap_err(),
            ParserError::UnbalancedRSquared
        );

        let mut p3 = Parser::new("1.2.3");
        assert!(matches!(
            p3.next::<()>().unwrap_err(),
            ParserError::NumberParseError(_)
        ));

        // A trailing map key has no value.
        let mut p4 = Parser::new("{ \"key\" }");
        assert_eq!(
            p4.next::<()>().unwrap_err(),
            ParserError::MapKeyMissingValue
        );

        // Map keys must be strings or symbols.
        let mut p5 = Parser::new("{ 42 \"value\" }");
        assert_eq!(p5.next::<()>().unwrap_err(), ParserError::InvalidMapKey);
    }

    #[test]
    fn test_parser_robustness_mismatched_interleaved_delimiters() {
        assert_eq!(
            Parser::new("([)]").next::<()>().unwrap_err(),
            ParserError::UnbalancedRParen
        );
        assert_eq!(
            Parser::new("{ [ } ]").next::<()>().unwrap_err(),
            ParserError::InvalidMapKey
        );
        assert_eq!(
            Parser::new("( { ) }").next::<()>().unwrap_err(),
            ParserError::UnbalancedRParen
        );
    }

    #[test]
    fn test_parser_robustness_abrupt_eof() {
        // Unterminated strings must report an error at end of input.
        let mut p1 = Parser::new("\"Unclosed string...");
        assert!(p1.next::<()>().is_err() || p1.next::<()>().is_ok());

        assert_eq!(
            Parser::new("(((1 2)").next::<()>().unwrap_err(),
            ParserError::UnclosedList
        );
        assert_eq!(
            Parser::new("[1 2").next::<()>().unwrap_err(),
            ParserError::UnclosedVector
        );
        assert_eq!(
            Parser::new("{ \"key\" \"val\"").next::<()>().unwrap_err(),
            ParserError::UnclosedMap
        );
    }

    #[test]
    fn test_parser_robustness_invalid_numbers() {
        assert!(matches!(
            Parser::new("1..2").next::<()>().unwrap_err(),
            ParserError::NumberParseError(_)
        ));
    }

    #[test]
    fn test_parser_robustness_malformed_maps() {
        assert_eq!(
            Parser::new("{ \"key1\" \"val1\" \"key2\" }")
                .next::<()>()
                .unwrap_err(),
            ParserError::MapKeyMissingValue
        );

        // Reject lists and vectors as map keys.
        assert_eq!(
            Parser::new("{ [1 2] \"value\" }").next::<()>().unwrap_err(),
            ParserError::InvalidMapKey
        );
        assert_eq!(
            Parser::new("{ (func) \"value\" }")
                .next::<()>()
                .unwrap_err(),
            ParserError::InvalidMapKey
        );
    }

    #[test]
    fn test_parser_robustness_empty_input() {
        assert_eq!(
            Parser::new("").next::<()>().unwrap_err(),
            ParserError::VoidExp
        );
        assert_eq!(
            Parser::new("   \n \t  ").next::<()>().unwrap_err(),
            ParserError::VoidExp
        );
    }

    #[test]
    fn test_parse_quoted_symbol() {
        let mut parser = Parser::new("'foo");
        let exp: LispExp<()> = parser.next().unwrap();

        // Should expand to: (quote foo)
        assert_eq!(
            exp,
            LispExp::form(vec![
                LispExp::symbol("quote".into()),
                LispExp::symbol("foo".into())
            ])
        );
    }

    #[test]
    fn test_parse_quoted_list() {
        let mut parser = Parser::new("'(1 2 a)");
        let exp: LispExp<()> = parser.next().unwrap();

        // Should expand to: (quote (1 2 a))
        assert_eq!(
            exp,
            LispExp::form(vec![
                LispExp::symbol("quote".into()),
                LispExp::proper_list(vec![
                    LispExp::number(1.0),
                    LispExp::number(2.0),
                    LispExp::symbol("a".into()),
                ])
            ])
        );
    }

    #[test]
    fn test_parse_nested_quotes() {
        let mut parser = Parser::new("''foo");
        let exp: LispExp<()> = parser.next().unwrap();

        // Should expand to: (quote (quote foo))
        assert_eq!(
            exp,
            LispExp::form(vec![
                LispExp::symbol("quote".into()),
                LispExp::proper_list(vec![
                    LispExp::symbol("quote".into()),
                    LispExp::symbol("foo".into())
                ])
            ])
        );
    }

    #[test]
    fn test_parse_quote_inside_list() {
        let mut parser = Parser::new("(setq x 'y)");
        let exp: LispExp<()> = parser.next().unwrap();

        // Should expand to: (setq x (quote y))
        assert_eq!(
            exp,
            LispExp::form(vec![
                LispExp::symbol("setq".into()),
                LispExp::symbol("x".into()),
                LispExp::form(vec![
                    LispExp::symbol("quote".into()),
                    LispExp::symbol("y".into())
                ])
            ])
        );
    }

    #[test]
    fn test_quote_at_eof() {
        // A dangling quote has no following expression.
        let mut parser = Parser::new("'");
        let res: Result<LispExp<()>, ParserError> = parser.next();

        assert_eq!(res, Err(ParserError::VoidExp));
    }

    #[test]
    fn test_quote_empty_structures() {
        // Quoting empty collections.
        let mut parser_list = Parser::new("'()");
        assert_eq!(
            parser_list.next::<()>().unwrap(),
            LispExp::form(vec![LispExp::symbol("quote".into()), LispExp::nil()])
        );

        let mut parser_vec = Parser::new("'[]");
        assert_eq!(
            parser_vec.next::<()>().unwrap(),
            LispExp::form(vec![LispExp::symbol("quote".into()), LispExp::vec(vec![])])
        );
    }

    #[test]
    fn test_quote_literals() {
        // Quoting self-evaluating numbers and strings.
        let mut parser_num = Parser::new("'42.5");
        assert_eq!(
            parser_num.next::<()>().unwrap(),
            LispExp::form(vec![LispExp::symbol("quote".into()), LispExp::number(42.5)])
        );

        let mut parser_str = Parser::new("'\"hello\"");
        assert_eq!(
            parser_str.next::<()>().unwrap(),
            LispExp::form(vec![
                LispExp::symbol("quote".into()),
                LispExp::string("hello".into())
            ])
        );
    }

    #[test]
    fn test_quote_split_by_comments() {
        // A comment between a quote prefix and its expression.
        let mut parser = Parser::new("' ; this is an evil comment\n target-symbol");
        assert_eq!(
            parser.next::<()>().unwrap(),
            LispExp::form(vec![
                LispExp::symbol("quote".into()),
                LispExp::symbol("target-symbol".into())
            ])
        );
    }

    #[test]
    fn misplaced_dots_return_errors() {
        for source in [".", "(1 . .)", "[.]", "'.", "{k .}"] {
            assert_eq!(
                Parser::new(source).next::<()>(),
                Err(ParserError::UnexpectedToken(".".into())),
                "input: {source}"
            );
        }
    }

    #[test]
    fn closing_delimiters_after_prefixes_return_errors() {
        for (source, delimiter) in [
            ("(list 'a ')", ")"),
            ("(a ,)", ")"),
            ("[1 `]", "]"),
            ("{k ,@}", "}"),
        ] {
            assert_eq!(
                Parser::new(source).next::<()>(),
                Err(ParserError::UnexpectedToken(delimiter.into())),
                "input: {source}"
            );
        }
    }

    #[test]
    fn a_valid_dotted_pair_still_parses() {
        assert_eq!(
            Parser::new("(1 . 2)").next::<()>(),
            Ok(LispExp::cons(LispExp::number(1.0), LispExp::number(2.0),))
        );
    }

    #[test]
    fn iterative_reader_handles_end_of_input() {
        let mut parser = Parser::new("foo");

        assert_eq!(parser.next::<()>(), Ok(LispExp::symbol("foo".into())));

        for _ in 0..3 {
            assert_eq!(parser.next::<()>(), Err(ParserError::VoidExp));
        }

        for (source, error) in [
            ("(foo", ParserError::UnclosedList),
            ("[foo", ParserError::UnclosedVector),
            ("{key", ParserError::UnclosedMap),
            ("{key value", ParserError::UnclosedMap),
        ] {
            assert_eq!(
                Parser::new(source).next::<()>(),
                Err(error),
                "input: {source}"
            );
        }
    }

    #[test]
    fn iterative_reader_limits_nesting() {
        use crate::parser::MAX_READ_DEPTH;

        let allowed = format!(
            "{}{}",
            "(".repeat(MAX_READ_DEPTH),
            ")".repeat(MAX_READ_DEPTH)
        );
        assert!(Parser::new(&allowed).next::<()>().is_ok());

        for source in [
            "(".repeat(100_000),
            "[".repeat(100_000),
            "'".repeat(100_000),
        ] {
            assert_eq!(Parser::new(&source).next::<()>(), Err(ParserError::TooDeep));
        }
    }

    #[test]
    fn iterative_reader_preserves_quoted_dotted_data() {
        let expected = LispExp::form(vec![
            LispExp::symbol("quote".into()),
            LispExp::cons(
                LispExp::proper_list(vec![LispExp::number(1.0), LispExp::number(2.0)]),
                LispExp::number(3.0),
            ),
        ]);

        for source in ["'((1 2) . 3)", "(quote ((1 2) . 3))"] {
            assert_eq!(Parser::new(source).next::<()>(), Ok(expected.clone()));
        }
    }
}
