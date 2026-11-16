#[cfg(test)]
mod test {
    use crate::{Parser, Token, parser::ParserError};

    fn tokenize(input: &str) -> Result<Vec<Token>, ParserError> {
        let mut parser = Parser::new(input);
        let mut ret = Vec::new();

        while let Some(token) = parser.next_token()? {
            ret.push(token);
        }

        return Ok(ret);
    }

    fn lex_all(input: &str) -> Result<Vec<Token>, ParserError> {
        let input_str = input.to_string();
        let mut parser = Parser::new(&input_str);
        let mut tokens = Vec::new();

        while let Some(token) = parser.next_token()? {
            tokens.push(token);
        }
        Ok(tokens)
    }

    #[test]
    fn test_symbols_and_spaces() {
        let input = "def  my-var   + ";
        let tokens = lex_all(input).unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Symbol("def".into()),
                Token::Symbol("my-var".into()),
                Token::Symbol("+".into()),
            ]
        );
    }

    #[test]
    fn symbol_reader_delimiters_preserve_adjacent_tokens() {
        for (suffix, following) in [
            (
                r#""value"tail"#,
                vec![Token::String("value".into()), Token::Symbol("tail".into())],
            ),
            ("'tail", vec![Token::Quote, Token::Symbol("tail".into())]),
            ("`tail", vec![Token::BackQuote, Token::Symbol("tail".into())]),
            (",tail", vec![Token::Comma, Token::Symbol("tail".into())]),
            (",@tail", vec![Token::CommaAt, Token::Symbol("tail".into())]),
        ] {
            for symbol in ["head", "123abc", "+foo", ".foo", "1efoo"] {
                let source = format!("{symbol}{suffix}");
                let mut expected = vec![Token::Symbol(symbol.into())];
                expected.extend(following.clone());
                assert_eq!(lex_all(&source), Ok(expected), "{source:?}");
            }
        }
    }

    #[test]
    fn symbol_cr_and_crlf_boundaries_preserve_comments() {
        for source in [
            "left\rright", "left\r\nright", "left; ignored\r\nright",
            "left\r; ignored\r\nright", "left; ignored\nright",
        ] {
            assert_eq!(
                lex_all(source),
                Ok(vec![
                    Token::Symbol("left".into()),
                    Token::Symbol("right".into()),
                ]),
                "{source:?}"
            );
        }
    }

    #[test]
    fn numeric_error_recovery_preserves_reader_delimiters() {
        for bad in ["1.2.3junk", "1e2.3junk"] {
            for suffix in [
                r#""value"tail"#, "'tail", "`tail", ",tail", ",@tail",
                "'; ignored\r\ntail", "; ignored\r\ntail", "\rtail", "\r\ntail",
                "()tail", "[]tail", "{}tail",
            ] {
                let source = format!("{bad}{suffix}");
                let mut parser = Parser::new(&source);
                assert!(matches!(
                    parser.next_token(),
                    Err(ParserError::NumberParseError(_))
                ));
                for token in lex_all(suffix).unwrap() {
                    assert_eq!(parser.next_token(), Ok(Some(token)), "{source:?}");
                }
                assert_eq!(parser.next_token(), Ok(None), "{source:?}");
            }
        }
    }

    #[test]
    fn test_strings() {
        let input = r#""hello world" "escaped \" quote""#;
        let tokens = lex_all(input).unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::String("hello world".into()),
                Token::String("escaped \" quote".into()),
            ]
        );
    }

    #[test]
    fn test_numbers() {
        let input = "123 45.67 0.1 ";
        let tokens = lex_all(input).unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Number(123.0),
                Token::Number(45.67),
                Token::Number(0.1),
            ]
        );
    }

    fn assert_atom_at_boundaries(atom: &str, expected: Token) {
        for suffix in ["", " ", "\t", "\n", "\r", "; comment", "()", "[]", "{}"] {
            let source = format!("{atom}{suffix}");
            let mut parser = Parser::new(&source);
            assert_eq!(parser.next_token(), Ok(Some(expected.clone())), "{source:?}");
            for token in lex_all(suffix).unwrap() {
                assert_eq!(parser.next_token(), Ok(Some(token)), "{source:?}");
            }
            for _ in 0..2 {
                assert_eq!(parser.next_token(), Ok(None), "{source:?}");
            }
        }
    }

    #[test]
    fn test_complete_number_grammar() {
        for (source, value) in [
            (".9", 0.9),
            ("+.9", 0.9),
            ("-.9", -0.9),
            ("+5", 5.0),
            ("-5", -5.0),
            ("5.", 5.0),
            ("1e3", 1000.0),
            ("1E+2", 100.0),
            ("2.5e-3", 0.0025),
            ("+.9e+2", 90.0),
            ("-.9E-2", -0.009),
            ("1.e2", 100.0),
            ("1.7976931348623157e308", f64::MAX),
            ("5e-324", f64::from_bits(1)),
            ("1e-999", 0.0),
        ] {
            assert_atom_at_boundaries(source, Token::Number(value));
        }
    }

    #[test]
    fn test_incomplete_exponents_and_alphanumeric_symbols() {
        for source in [
            "+", "-", "1+", "1-", "1-2", "inf", "NaN", "e5", "abc123",
            "123abc", "1.5a", "+5foo", ".9foo", "1e3foo", "1e309foo", "1e", "1E",
            "1e+", "1e-", "+1E+", "-.9e-", "1.e", "1e++2",
        ] {
            assert_atom_at_boundaries(source, Token::Symbol(source.into()));
        }
    }

    #[test]
    fn test_nonfinite_numbers_are_rejected_without_losing_delimiters() {
        let huge_integer = "9".repeat(400);
        let huge_decimal = format!("{huge_integer}.0");
        for atom in ["1e309", "-1e309", "+1E309", "1.8e308", &huge_integer, &huge_decimal] {
            for suffix in ["", " ", "; comment\n7", "() 7", "[] 7", "{} 7"] {
                let source = format!("{atom}{suffix}");
                let mut parser = Parser::new(&source);
                assert_eq!(
                    parser.next_token(),
                    Err(ParserError::NumberParseError(atom.into())),
                    "{source:?}"
                );
                for token in lex_all(suffix).unwrap() {
                    assert_eq!(parser.next_token(), Ok(Some(token)), "{source:?}");
                }
                for _ in 0..2 {
                    assert_eq!(parser.next_token(), Ok(None), "{source:?}");
                }
            }
        }
    }

    #[test]
    fn test_lisp_expression() {
        let input = "(define (add x y) (+ x y)) ";
        let tokens = lex_all(input).unwrap();
        let expected = vec![
            Token::LParen,
            Token::Symbol("define".into()),
            Token::LParen,
            Token::Symbol("add".into()),
            Token::Symbol("x".into()),
            Token::Symbol("y".into()),
            Token::RParen,
            Token::LParen,
            Token::Symbol("+".into()),
            Token::Symbol("x".into()),
            Token::Symbol("y".into()),
            Token::RParen,
            Token::RParen,
        ];
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_unbalanced_error() {
        let input = "( ] )";
        let result = lex_all(input);
        match result {
            Err(ParserError::UnbalancedRSquared) => (),
            _ => panic!("Expected UnbalancedRSquared error"),
        }
    }

    #[test]
    fn test_eof_handling_for_numbers() {
        let input = "42";
        let _ = lex_all(input);
    }

    #[test]
    fn test_delimiters_no_spaces() {
        let input = "(factorial 5)(add x y)";
        let tokens = lex_all(input).unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::LParen,
                Token::Symbol("factorial".into()),
                Token::Number(5.0),
                Token::RParen,
                Token::LParen,
                Token::Symbol("add".into()),
                Token::Symbol("x".into()),
                Token::Symbol("y".into()),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn test_complex_strings() {
        let input = r#" "line1\nline2" "quote: \" " "" "#;
        let tokens = lex_all(input).unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::String("line1\nline2".into()),
                Token::String("quote: \" ".into()),
                Token::String("".into()),
            ]
        );
    }

    /// Named string escapes decode to their corresponding characters.
    #[test]
    fn the_named_escapes_become_the_characters_they_name() {
        let input = r#" "a\nb" "a\tb" "a\rb" "a\0b" "a\eb" "#;
        assert_eq!(
            lex_all(input).unwrap(),
            vec![
                Token::String("a\nb".into()),
                Token::String("a\tb".into()),
                Token::String("a\rb".into()),
                Token::String("a\0b".into()),
                Token::String("a\u{1b}b".into()),
            ]
        );
    }

    /// Preserve unknown escapes such as `\b` for downstream regex parsers.
    #[test]
    fn an_unknown_escape_keeps_its_backslash() {
        assert_eq!(
            lex_all(r#" "\b(fn|let)" "\s*" "\." "#).unwrap(),
            vec![
                Token::String("\\b(fn|let)".into()),
                Token::String("\\s*".into()),
                Token::String("\\.".into()),
            ]
        );
    }

    /// A doubled backslash decodes once; a following `n` remains literal.
    #[test]
    fn a_doubled_backslash_is_one_backslash() {
        assert_eq!(
            lex_all(r#" "\\b" "\\n" "#).unwrap(),
            vec![Token::String("\\b".into()), Token::String("\\n".into())]
        );
    }

    #[test]
    fn test_deep_nesting() {
        let input = "(let [x {y 10}] (fn (z) [z x]))";
        let tokens = lex_all(input).unwrap();

        let expected = vec![
            Token::LParen,
            Token::Symbol("let".into()),
            Token::LSquared,
            Token::Symbol("x".into()),
            Token::LBracket,
            Token::Symbol("y".into()),
            Token::Number(10.0),
            Token::RBracket,
            Token::RSquared,
            Token::LParen,
            Token::Symbol("fn".into()),
            Token::LParen,
            Token::Symbol("z".into()),
            Token::RParen,
            Token::LSquared,
            Token::Symbol("z".into()),
            Token::Symbol("x".into()),
            Token::RSquared,
            Token::RParen,
            Token::RParen,
        ];
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_numeric_edge_cases() {
        let input = "123.456 0.001 100 ";
        let tokens = lex_all(input).unwrap();
        assert_eq!(tokens[0], Token::Number(123.456));
        assert_eq!(tokens[1], Token::Number(0.001));
        assert_eq!(tokens[2], Token::Number(100.0));

        let input_err = "1.2.3 ";
        let result = lex_all(input_err);
        assert!(matches!(result, Err(ParserError::NumberParseError(_))));
    }

    #[test]
    fn test_tokenize_basic() {
        let input = "(+ 1 2.5 \"hello\")";
        let tokens = tokenize(input).expect("Failed to tokenize");

        assert_eq!(
            tokens,
            vec![
                Token::LParen,
                Token::Symbol("+".to_string()),
                Token::Number(1.0),
                Token::Number(2.5),
                Token::String("hello".to_string()),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn test_tokenize_complex_symbols() {
        let input = "(insert-text! my-var)";
        let tokens = tokenize(input).unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::LParen,
                Token::Symbol("insert-text!".to_string()),
                Token::Symbol("my-var".to_string()),
                Token::RParen,
            ]
        );
    }
}
