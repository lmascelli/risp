//! Boundary tests for the standard primitives installed by `setup_base_env`.
#[cfg(test)]
mod tests {
    use crate::{Env, EvalError, LispExp, Parser, eval, setup_base_env};
    use std::sync::Arc;

    fn env_with_primitives() -> Arc<Env<()>> {
        let env = Env::new_root();
        setup_base_env(env.clone());
        env
    }

    fn eval_str(source: &str, env: Arc<Env<()>>) -> Result<LispExp<()>, EvalError<()>> {
        let wrapped = format!("(progn {})", source);
        let mut parser = Parser::new(&wrapped);
        let ast = parser.next().expect("failed to parse test script");
        eval(&ast, env, &())
    }

    fn eval_ok(source: &str) -> LispExp<()> {
        eval_str(source, env_with_primitives()).unwrap_or_else(|e| {
            panic!("eval of `{source}` failed: {e:?}");
        })
    }

    fn eval_err(source: &str) -> EvalError<()> {
        eval_str(source, env_with_primitives()).expect_err(&format!("expected `{source}` to fail"))
    }

    // Lists: dotted pairs, append, out-of-range access

    #[test]
    fn car_and_cdr_work_on_a_dotted_pair() {
        assert_eq!(eval_ok("(car (cons 1 2))"), LispExp::number(1.0));
        assert_eq!(eval_ok("(cdr (cons 1 2))"), LispExp::number(2.0));
    }

    #[test]
    fn cons_onto_a_dotted_pair_extends_the_proper_list_prefix() {
        // (cons 1 (cons 2 3)) => (1 2 . 3)
        assert_eq!(
            eval_ok("(cons 1 (cons 2 3))"),
            LispExp::improper_list(
                vec![LispExp::number(1.0), LispExp::number(2.0)],
                LispExp::number(3.0)
            )
        );
    }

    #[test]
    fn append_with_a_non_list_final_argument_produces_a_dotted_list() {
        assert_eq!(
            eval_ok("(append '(1 2) 3)"),
            LispExp::improper_list(
                vec![LispExp::number(1.0), LispExp::number(2.0)],
                LispExp::number(3.0)
            )
        );
    }

    #[test]
    fn append_with_no_arguments_returns_nil() {
        assert_eq!(eval_ok("(append)"), LispExp::nil());
    }

    #[test]
    fn nth_and_elt_out_of_range_or_negative_return_nil_not_an_error() {
        assert_eq!(eval_ok("(nth 5 '(1 2 3))"), LispExp::nil());
        assert_eq!(eval_ok("(nth -1 '(1 2 3))"), LispExp::nil());
        assert_eq!(eval_ok("(elt \"hi\" 5)"), LispExp::nil());
        assert_eq!(eval_ok("(elt \"hi\" -1)"), LispExp::nil());
    }

    #[test]
    fn nthcdr_at_or_past_the_end_returns_nil() {
        assert_eq!(eval_ok("(nthcdr 3 '(1 2 3))"), LispExp::nil());
        assert_eq!(eval_ok("(nthcdr 99 '(1 2 3))"), LispExp::nil());
    }

    #[test]
    fn macro_arguments_are_nested_data_without_evaluation() {
        assert_eq!(
            eval_ok("\
                (defmacro nested-tail (form)
                  (list 'quote (cdr (car (cdr form)))))
                (nested-tail (never-called (also-never-called unbound 42)))
            "),
            eval_ok("'(unbound 42)")
        );
    }

    #[test]
    fn macro_arguments_preserve_quote_and_dotted_payloads() {
        for argument in ["'(a . b)", "(quote (a . b))"] {
            assert_eq!(
                eval_ok(&format!("\
                    (defmacro inspect-quote (form)
                      (list 'quote (list (car form) (car (cdr form)))))
                    (inspect-quote {argument})
                ")),
                eval_ok("'(quote (a . b))")
            );
        }
    }

    #[test]
    fn macro_optional_and_rest_arguments_are_data() {
        let definition = "\
            (defmacro arg-heads (a &optional b &rest more)
              (list 'quote (list (car a) (car b)
                                (car (car more)) (car (car (cdr more))))))
        ";
        for (arguments, expected) in [
            ("(required unbound)", "'(required nil nil nil)"),
            ("(required unbound) (optional unbound)", "'(required optional nil nil)"),
            (
                "(required unbound) (optional unbound) (first-rest unbound) (second-rest unbound)",
                "'(required optional first-rest second-rest)",
            ),
        ] {
            assert_eq!(
                eval_ok(&format!("{definition} (arg-heads {arguments})")),
                eval_ok(expected)
            );
        }
    }

    #[test]
    fn macro_destructured_expansion_runs_in_the_caller() {
        assert_eq!(
            eval_ok("\
                (let ((x 100))
                  (defmacro add-captured (form)
                    (list '+ x (car (cdr form))))
                  (let ((x 7)) (add-captured (ignored (+ x 1)))))
            "),
            LispExp::number(108.0)
        );
    }

    #[test]
    fn macro_generated_functions_accept_empty_parameter_lists() {
        assert_eq!(
            eval_ok("\
                (defmacro define-command (name params &rest body)
                  `(defun ,name ,params ,@body))
                (define-command answer () 42)
                (answer)
            "),
            LispExp::number(42.0)
        );
    }

    #[test]
    fn macro_generated_lambdas_accept_empty_parameter_lists() {
        assert_eq!(
            eval_ok("\
                (defmacro make-function (params &rest body)
                  `(lambda ,params ,@body))
                (funcall (make-function () 42))
            "),
            LispExp::number(42.0)
        );
    }

    #[test]
    fn macro_generated_macros_accept_empty_parameter_lists() {
        assert_eq!(
            eval_ok("\
                (defmacro define-macro (name params &rest body)
                  `(defmacro ,name ,params ,@body))
                (define-macro answer () 42)
                (answer)
            "),
            LispExp::number(42.0)
        );
    }

    // Arithmetic: division by zero, unary quirks, chaining, mod sign

    #[test]
    fn division_with_a_single_argument_returns_the_reciprocal() {
        assert_eq!(eval_ok("(/ 4)"), LispExp::number(0.25));
    }

    #[test]
    fn division_and_mod_by_zero_signal_a_runtime_error() {
        assert!(matches!(eval_err("(/ 1 0)"), EvalError::RuntimeMessage(_)));
        assert!(matches!(eval_err("(/ 0)"), EvalError::RuntimeMessage(_)));
        assert!(matches!(
            eval_err("(mod 1 0)"),
            EvalError::RuntimeMessage(_)
        ));
    }

    #[test]
    fn sum_with_no_arguments_returns_zero() {
        assert_eq!(eval_ok("(+)"), LispExp::number(0.0));
    }

    #[test]
    fn subtraction_with_a_single_argument_negates_it() {
        assert_eq!(eval_ok("(- 5)"), LispExp::number(-5.0));
        assert_eq!(eval_ok("(- -5)"), LispExp::number(5.0));
    }

    #[test]
    fn mod_result_takes_the_sign_of_the_divisor() {
        assert_eq!(eval_ok("(mod 7 3)"), LispExp::number(1.0));
        assert_eq!(eval_ok("(mod 7 -3)"), LispExp::number(-2.0));
        assert_eq!(eval_ok("(mod -7 3)"), LispExp::number(2.0));
        assert_eq!(eval_ok("(mod -7 -3)"), LispExp::number(-1.0));
    }

    #[test]
    fn chained_relational_comparisons_check_every_adjacent_pair() {
        assert_eq!(eval_ok("(< 1 2 3)"), LispExp::t());
        assert_eq!(eval_ok("(< 1 3 2)"), LispExp::nil());
        assert_eq!(eval_ok("(<= 1 1 2)"), LispExp::t());
    }

    #[test]
    fn numeric_equality_requires_every_argument_to_match() {
        assert_eq!(eval_ok("(= 1 1 1)"), LispExp::t());
        assert_eq!(eval_ok("(= 1 1 2)"), LispExp::nil());
    }

    // Strings: negative substring indices, format directives

    #[test]
    fn substring_negative_indices_count_from_the_end() {
        assert_eq!(
            eval_ok("(substring \"hello\" -3)"),
            LispExp::string("llo".into())
        );
        assert_eq!(
            eval_ok("(substring \"hello\" -3 -1)"),
            LispExp::string("ll".into())
        );
    }

    #[test]
    fn substring_start_after_end_is_a_runtime_error() {
        assert!(matches!(
            eval_err("(substring \"hi\" 1 0)"),
            EvalError::RuntimeMessage(_)
        ));
    }

    #[test]
    fn format_percent_percent_is_a_literal_percent() {
        assert_eq!(
            eval_ok("(format \"100%%\")"),
            LispExp::string("100%".into())
        );
    }

    #[test]
    fn format_running_out_of_arguments_is_an_error() {
        assert!(matches!(
            eval_err("(format \"%s\")"),
            EvalError::WrongNumberOfArguments { .. }
        ));
    }

    #[test]
    fn split_string_defaults_to_whitespace_and_drops_empty_pieces() {
        assert_eq!(
            eval_ok("(split-string \"  a  b c \")"),
            LispExp::proper_list(vec![
                LispExp::string("a".into()),
                LispExp::string("b".into()),
                LispExp::string("c".into()),
            ])
        );
    }

    #[test]
    fn split_string_with_empty_separator_splits_into_characters() {
        assert_eq!(
            eval_ok("(split-string \"ab\" \"\")"),
            LispExp::proper_list(vec![
                LispExp::string("a".into()),
                LispExp::string("b".into())
            ])
        );
    }

    #[test]
    fn string_trim_takes_the_whitespace_off_both_ends_and_keeps_the_inside() {
        assert_eq!(
            eval_ok("(string-trim \" \\t a  b \\n\")"),
            LispExp::string("a  b".into())
        );
        assert_eq!(eval_ok("(string-trim \"ab\")"), LispExp::string("ab".into()));
    }

    #[test]
    fn string_trim_of_nothing_but_whitespace_is_empty() {
        assert_eq!(eval_ok("(string-trim \"  \\t\\n \")"), LispExp::string("".into()));
        assert_eq!(eval_ok("(string-trim \"\")"), LispExp::string("".into()));
    }

    #[test]
    fn string_trim_knows_the_whitespace_split_string_splits_on() {
        // An ideographic space and a no-break space are Unicode white space.
        assert_eq!(
            eval_ok("(string-trim \"\u{3000}città\u{a0}\")"),
            LispExp::string("città".into())
        );
        assert_eq!(
            eval_ok("(split-string \"\u{3000}città\u{a0}\")"),
            LispExp::proper_list(vec![LispExp::string("città".into())])
        );
    }

    #[test]
    fn string_trim_wants_one_string() {
        assert!(matches!(
            eval_err("(string-trim)"),
            EvalError::WrongNumberOfArguments { .. }
        ));
        assert!(matches!(
            eval_err("(string-trim \"a\" \"b\")"),
            EvalError::WrongNumberOfArguments { .. }
        ));
        assert!(matches!(
            eval_err("(string-trim 5)"),
            EvalError::WrongArgumentType { .. }
        ));
    }

    #[test]
    fn string_trim_left_and_right_each_take_one_end_only() {
        assert_eq!(
            eval_ok("(string-trim-left \" \\t a b \\n\")"),
            LispExp::string("a b \n".into())
        );
        assert_eq!(
            eval_ok("(string-trim-right \" \\t a b \\n\")"),
            LispExp::string(" \t a b".into())
        );
    }

    #[test]
    fn string_trim_left_and_right_of_nothing_but_whitespace_are_empty() {
        for name in ["string-trim-left", "string-trim-right"] {
            assert_eq!(
                eval_ok(&format!("({name} \"  \\t\\n \")")),
                LispExp::string("".into()),
                "{name}"
            );
            assert_eq!(
                eval_ok(&format!("({name} \"\")")),
                LispExp::string("".into()),
                "{name}"
            );
        }
    }

    #[test]
    fn string_trim_is_string_trim_left_of_string_trim_right() {
        // The three agree on what whitespace is, Unicode's included.
        assert_eq!(
            eval_ok("(string-trim-left (string-trim-right \"\u{3000} città \u{a0}\"))"),
            eval_ok("(string-trim \"\u{3000} città \u{a0}\")")
        );
        assert_eq!(
            eval_ok("(string-trim-left \"\u{3000}città\")"),
            LispExp::string("città".into())
        );
        assert_eq!(
            eval_ok("(string-trim-right \"città\u{a0}\")"),
            LispExp::string("città".into())
        );
    }

    #[test]
    fn string_trim_left_and_right_want_one_string() {
        for name in ["string-trim-left", "string-trim-right"] {
            assert!(
                matches!(
                    eval_err(&format!("({name})")),
                    EvalError::WrongNumberOfArguments { .. }
                ),
                "{name}"
            );
            assert!(
                matches!(
                    eval_err(&format!("({name} \"a\" \"b\")")),
                    EvalError::WrongNumberOfArguments { .. }
                ),
                "{name}"
            );
            assert!(
                matches!(
                    eval_err(&format!("({name} 5)")),
                    EvalError::WrongArgumentType { .. }
                ),
                "{name}"
            );
        }
    }

    #[test]
    fn number_to_string_omits_the_decimal_point_for_integral_values() {
        assert_eq!(eval_ok("(number-to-string 5)"), LispExp::string("5".into()));
        assert_eq!(
            eval_ok("(number-to-string 5.5)"),
            LispExp::string("5.5".into())
        );
    }

    #[test]
    fn string_to_number_returns_zero_for_unparseable_input() {
        assert_eq!(
            eval_ok("(string-to-number \"not-a-number\")"),
            LispExp::number(0.0)
        );
    }

    // Calling conventions: funcall vs. apply/mapcar symbol resolution

    #[test]
    fn computed_primitive_evaluates_head_and_arguments_once_like_funcall() {
        let env = env_with_primitives();
        env.set_variable("op".into(), env.get_function("list").unwrap());
        for call in [
            "((progn (setq heads (+ heads 1)) op)",
            "(funcall (progn (setq heads (+ heads 1)) op)",
        ] {
            assert_eq!(
                eval_str(
                    &format!("\
                        (setq heads 0 n 0)
                        (setq result {call}
                          (progn (setq n (+ n 1)) n)
                          (progn (setq n (+ n 1)) n)
                          'unbound '(missing-function) 'nil))
                        (list result heads n)
                    "),
                    env.clone(),
                ),
                Ok(eval_ok("'((1 2 unbound (missing-function) nil) 1 2)")),
            );
        }
    }

    #[test]
    fn computed_primitive_reports_the_same_errors_as_funcall() {
        let env = env_with_primitives();
        env.set_variable("op".into(), env.get_function("car").unwrap());
        for args in ["", "1", "nil nil", "unbound"] {
            let result = eval_str(&format!("((if t op nil) {args})"), env.clone());
            assert!(result.is_err());
            assert_eq!(
                result,
                eval_str(&format!("(funcall op {args})"), env.clone()),
            );
        }
    }

    #[test]
    fn computed_primitive_receives_the_callers_environment_like_funcall() {
        let env = env_with_primitives();
        env.set_variable("op".into(), env.get_function("eval").unwrap());
        for call in ["((if t op nil) '(+ x 2))", "(funcall op '(+ x 2))"] {
            assert_eq!(
                eval_str(&format!("(let ((x 40)) {call})"), env.clone()),
                Ok(LispExp::number(42.0)),
            );
        }
    }

    #[test]
    fn computed_primitive_preserves_computed_lambda_tail_calls() {
        let env = env_with_primitives();
        env.set_variable("add".into(), env.get_function("+").unwrap());
        env.set_variable("subtract".into(), env.get_function("-").unwrap());
        assert_eq!(
            eval_str(
                "(setq countdown
                   (lambda (n)
                     (if (= n 0)
                         ((if t add nil) 20 22)
                         ((if t countdown nil) ((if t subtract nil) n 1)))))
                 ((if t countdown nil) 10000)",
                env,
            ),
            Ok(LispExp::number(42.0)),
        );
    }

    #[test]
    fn funcall_apply_and_mapcar_all_resolve_a_symbol_argument() {
        // All three entry points must resolve function symbols consistently.
        assert_eq!(eval_ok("(funcall 'car '(1 2))"), LispExp::number(1.0));
        assert_eq!(eval_ok("(apply 'car '((1 2)))"), LispExp::number(1.0));
        assert_eq!(
            eval_ok("(mapcar 'car '((1 2) (3 4)))"),
            LispExp::proper_list(vec![LispExp::number(1.0), LispExp::number(3.0)])
        );
    }

    #[test]
    fn add_to_list_requires_an_existing_list_variable() {
        assert!(matches!(
            eval_err("(add-to-list 'undefined-list 1)"),
            EvalError::RuntimeMessage(_)
        ));
    }

    #[test]
    fn list_mutations_skip_duplicate_arguments_in_first_occurrence_order() {
        for (initial, arguments, prepended, appended) in [
            ("nil", "1 2 1 3 2", "(3 2 1)", "(1 2 3)"),
            (
                "'(1 2 3 2)",
                "2 4 5 4 3 5",
                "(5 4 1 2 3 2)",
                "(1 2 3 2 4 5)",
            ),
            (
                "'((1) (1))",
                "'(1) '(2) '(3) '(2)",
                "((3) (2) (1) (1))",
                "((1) (1) (2) (3))",
            ),
        ] {
            for (primitive, expected) in [
                ("add-to-list", prepended),
                ("append-to-list", appended),
            ] {
                assert_eq!(
                    eval_ok(&format!("\
                        (setq items {initial})
                        (list ({primitive} 'items {arguments}) items
                              ({primitive} 'items {arguments}) items)
                    ")),
                    eval_ok(&format!("'(items {expected} items {expected})")),
                    "{primitive}: {initial}, {arguments}"
                );
            }
        }
    }

    #[test]
    fn remove_from_list_names_itself_in_invalid_list_errors() {
        for setup in ["", "(setq items 7)"] {
            match eval_err(&format!("{setup} (remove-from-list 'items 1)")) {
                EvalError::RuntimeMessage(message) => assert_eq!(
                    message,
                    "[ERROR] remove-from-list items is not an existing list"
                ),
                other => panic!("expected a runtime message, got {other:?}"),
            }
        }
    }

    const LIST_MUTATION_CASES: [(&str, &str); 3] = [
        ("add-to-list", "(5 4 1 2 3 2)"),
        ("append-to-list", "(1 2 3 2 4 5)"),
        ("remove-from-list", "(1)"),
    ];

    #[test]
    fn list_mutations_update_global_binding_and_return_symbol() {
        for (primitive, expected) in LIST_MUTATION_CASES {
            assert_eq!(
                eval_ok(&format!("\
                    (setq items '(1 2 3 2))
                    (list ({primitive} 'items 2 3 4 5) items)
                ")),
                eval_ok(&format!("'(items {expected})")),
                "{primitive}"
            );
        }
    }

    #[test]
    fn list_mutations_update_global_binding_from_nested_scope() {
        for (primitive, expected) in LIST_MUTATION_CASES {
            assert_eq!(
                eval_ok(&format!("\
                    (setq items '(1 2 3 2))
                    (list
                      (let ((outer 0))
                        (let ((inner 0)) ({primitive} 'items 2 3 4 5)))
                      items)
                ")),
                eval_ok(&format!("'(items {expected})")),
                "{primitive}"
            );
        }
    }

    #[test]
    fn list_mutations_update_nearest_shadowing_binding() {
        for (primitive, expected) in LIST_MUTATION_CASES {
            for call in [
                format!("({primitive} 'items 2 3 4 5)"),
                format!("(let ((inner 0)) ({primitive} 'items 2 3 4 5))"),
            ] {
                assert_eq!(
                    eval_ok(&format!("\
                        (setq items '(global))
                        (list
                          (let ((items '(1 2 3 2))) (list {call} items))
                          items)
                    ")),
                    eval_ok(&format!("'((items {expected}) (global))")),
                    "{call}"
                );
            }
        }
    }

    #[test]
    fn member_and_memq_return_the_original_tail() {
        for search in ["member", "memq"] {
            for list in ["(1 2 3 2)", "(1 2 3 . 4)"] {
                assert_eq!(
                    eval_ok(&format!(
                        "(let ((xs '{list}))
                           (list (eq ({search} 1 xs) xs)
                                 (eq ({search} 2 xs) (cdr xs))
                                 (eq ({search} 3 xs) (cdr (cdr xs)))))"
                    )),
                    eval_ok("'(t t t)"),
                    "{search}: {list}"
                );
            }
        }
    }

    #[test]
    fn member_and_memq_preserve_list_boundaries() {
        for search in ["member", "memq"] {
            for args in ["9 '(1 2 3)", "4 '(1 2 3 . 4)", "1 nil"] {
                assert_eq!(eval_ok(&format!("({search} {args})")), LispExp::nil());
            }
            assert!(matches!(
                eval_err(&format!("({search} 1 7)")),
                EvalError::WrongArgumentType { expected, got }
                    if expected == "List" && got == LispExp::number(7.0)
            ));
            assert!(matches!(
                eval_err(&format!("({search} 1)")),
                EvalError::WrongNumberOfArguments { expected: 2, got: 1 }
            ));
        }
    }

    #[test]
    fn searches_distinguish_identity_from_structural_equality() {
        for value in [
            "(list 1 2)",
            "(cons 1 2)",
            "(vector 1 2)",
            "(make-map 'key 1)",
            "\"ab\"",
        ] {
            let setup = format!("(setq x {value}) (setq y {value})");
            assert_eq!(
                eval_ok(&format!("{setup} (list (eq x y) (equal x y))")),
                eval_ok("'(nil t)")
            );
            for (search, equal_search, collection, expected) in [
                ("memq", "member", "(list y 9)", "(list y 9)"),
                ("assq", "assoc", "(list (cons y 7))", "(cons y 7)"),
            ] {
                for (call, result) in [
                    (format!("({search} x {collection})"), "nil"),
                    (format!("({equal_search} x {collection})"), expected),
                    (format!("({search} y {collection})"), expected),
                ] {
                    assert_eq!(
                        eval_ok(&format!("{setup} {call}")),
                        eval_ok(&format!("{setup} {result}")),
                        "{value}: {call}"
                    );
                }
            }
            assert_eq!(
                eval_ok(&format!("{setup} (memq x (list y x 9))")),
                eval_ok(&format!("{setup} (list x 9)"))
            );
            assert_eq!(
                eval_ok(&format!("{setup} (assq x (list (cons y 1) (cons x 2)))")),
                eval_ok(&format!("{setup} (cons x 2)"))
            );
            assert_eq!(
                eval_ok(&format!("{setup} (eq (car (memq x (list x))) x)")),
                LispExp::t()
            );
            assert_eq!(
                eval_ok(&format!(
                    "{setup} (setq entry (cons x 7)) (eq (assq x (list entry)) entry)"
                )),
                LispExp::t()
            );
        }
    }

    #[test]
    fn searches_use_eq_value_rules_for_immediate_values() {
        for value in ["42", "'key", "nil"] {
            assert_eq!(
                eval_ok(&format!("(memq {value} (list {value} 9))")),
                eval_ok(&format!("(list {value} 9)"))
            );
            assert_eq!(
                eval_ok(&format!("(assq {value} (list nil (cons {value} 9)))")),
                eval_ok(&format!("(cons {value} 9)"))
            );
        }
    }

    // Predicates: eq/eql identity vs. equal structural comparison,
    // listp/functionp resolving nil and function-bound symbols

    #[test]
    fn eq_is_identity_based_while_equal_is_structural() {
        // Two independently-built lists with the same contents are `equal`
        // but not `eq` -- they're different allocations.
        assert_eq!(eval_ok("(eq (list 1 2) (list 1 2))"), LispExp::nil());
        assert_eq!(eval_ok("(equal (list 1 2) (list 1 2))"), LispExp::t());
    }

    #[test]
    fn eq_treats_numbers_and_symbols_as_immediate_values() {
        // Numbers and symbols compare by value under all three equality predicates.
        assert_eq!(eval_ok("(eq 1 1)"), LispExp::t());
        assert_eq!(eval_ok("(eq 'foo 'foo)"), LispExp::t());
        assert_eq!(eval_ok("(eq 1 2)"), LispExp::nil());
    }

    #[test]
    fn eq_on_a_list_bound_to_two_variables_is_the_same_allocation() {
        // Both variables share the same list allocation.
        let script = "(setq x (list 1 2)) (setq y x) (eq x y)";
        assert_eq!(eval_ok(script), LispExp::t());
    }

    #[test]
    fn listp_recognizes_nil_as_a_list() {
        assert_eq!(eval_ok("(listp nil)"), LispExp::t());
        assert_eq!(eval_ok("(listp '(1))"), LispExp::t());
        assert_eq!(eval_ok("(listp 5)"), LispExp::nil());
    }

    #[test]
    fn functionp_resolves_a_function_bound_symbol() {
        assert_eq!(eval_ok("(functionp 'car)"), LispExp::t());
        assert_eq!(eval_ok("(functionp (lambda () 1))"), LispExp::t());
        assert_eq!(eval_ok("(functionp 'no-such-function)"), LispExp::nil());
        // A variable-only binding doesn't count.
        assert_eq!(eval_ok("(setq x 1) (functionp 'x)"), LispExp::nil());
    }

    #[test]
    fn functionp_accepts_callables_and_alias_chains_without_executing_them() {
        let env = env_with_primitives();
        let callables = [
            eval_str("(lambda () (error \"must not execute\"))", env.clone()).unwrap(),
            LispExp::primitive(|_, _, _| panic!("must not execute primitive"), None),
            LispExp::native(|_, _, _| panic!("must not execute native"), None),
        ];
        env.set_function("alias".into(), LispExp::symbol("target".into()));
        env.set_function("chain".into(), LispExp::symbol("alias".into()));

        for callable in callables {
            env.set_variable("value".into(), callable.clone());
            env.set_function("target".into(), callable);
            for source in [
                "(functionp value)",
                "(functionp 'target)",
                "(functionp 'alias)",
                "(functionp 'chain)",
            ] {
                assert_eq!(eval_str(source, env.clone()), Ok(LispExp::t()), "{source}");
            }
        }
    }

    #[test]
    fn functionp_rejects_noncallable_bindings_and_missing_alias_targets() {
        let env = env_with_primitives();
        env.set_function("alias".into(), LispExp::symbol("target".into()));
        env.set_function("chain".into(), LispExp::symbol("alias".into()));

        for value in [
            LispExp::number(42.0),
            LispExp::nil(),
            LispExp::string("not callable".into()),
            LispExp::proper_list(vec![LispExp::number(1.0)]),
            LispExp::symbol("missing".into()),
        ] {
            env.set_variable("value".into(), value.clone());
            env.set_function("target".into(), value);
            for source in [
                "(functionp value)",
                "(functionp 'target)",
                "(functionp 'alias)",
                "(functionp 'chain)",
            ] {
                assert_eq!(eval_str(source, env.clone()), Ok(LispExp::nil()), "{source}");
            }
        }
    }

    #[test]
    fn functionp_rejects_self_and_mutual_alias_cycles() {
        let env = env_with_primitives();
        for (name, target) in [("self", "self"), ("a", "b"), ("b", "a"), ("prefix", "a")] {
            env.set_function(name.into(), LispExp::symbol(target.into()));
        }
        for source in [
            "(functionp 'self)",
            "(functionp 'a)",
            "(functionp 'b)",
            "(functionp 'prefix)",
        ] {
            assert_eq!(eval_str(source, env.clone()), Ok(LispExp::nil()), "{source}");
        }
    }

    #[test]
    fn predicates_agree_with_the_actual_runtime_type() {
        assert_eq!(eval_ok("(consp '(1))"), LispExp::t());
        assert_eq!(eval_ok("(consp nil)"), LispExp::nil());
        assert_eq!(eval_ok("(stringp \"x\")"), LispExp::t());
        assert_eq!(eval_ok("(stringp 1)"), LispExp::nil());
    }

    // `atom` tests for a non-cons value; `make-atom` creates a mutable container.

    #[test]
    fn atom_is_the_real_elisp_non_cons_predicate() {
        assert_eq!(eval_ok("(atom 5)"), LispExp::t());
        assert_eq!(eval_ok("(atom nil)"), LispExp::t());
        assert_eq!(eval_ok("(atom '(1 2))"), LispExp::nil());
    }

    #[test]
    fn make_atom_builds_a_mutable_reference_cell() {
        let script = "(setq r (make-atom 1)) (reset r 2) (deref r)";
        assert_eq!(eval_ok(script), LispExp::number(2.0));
    }

    #[test]
    fn fixed_arity_atom_and_fiber_primitives_reject_missing_or_excess_arguments() {
        for (source, expected, got) in [
            ("(make-atom)", 1, 0),
            ("(make-atom 1 2)", 1, 2),
            ("(deref)", 1, 0),
            ("(deref (make-atom 1) 2)", 1, 2),
            ("(reset)", 2, 0),
            ("(reset (make-atom 1))", 2, 1),
            ("(reset (make-atom 1) 2 3)", 2, 3),
            ("(resume)", 1, 0),
            ("(resume (fiber 1) 2)", 1, 2),
            ("(fiber-done-p)", 1, 0),
            ("(fiber-done-p (fiber 1) 2)", 1, 2),
        ] {
            assert_eq!(
                eval_err(source),
                EvalError::WrongNumberOfArguments { expected, got },
                "{source}"
            );
        }
    }

    #[test]
    fn fixed_arity_rejection_preserves_atom_and_fiber_state() {
        let env = env_with_primitives();
        eval_str("(setq a (make-atom 1) f (fiber (yield 2) 3))", env.clone()).unwrap();
        for (source, expected, got) in [("(reset a 2 3)", 2, 3), ("(resume f 0)", 1, 2)] {
            assert_eq!(
                eval_str(source, env.clone()),
                Err(EvalError::WrongNumberOfArguments { expected, got }),
                "{source}"
            );
        }
        assert_eq!(eval_str("(deref a)", env.clone()), Ok(LispExp::number(1.0)));
        assert_eq!(eval_str("(fiber-done-p f)", env.clone()), Ok(LispExp::nil()));
        assert_eq!(eval_str("(resume f)", env.clone()), Ok(LispExp::number(2.0)));
        assert_eq!(eval_str("(resume f)", env.clone()), Ok(LispExp::number(3.0)));
        assert_eq!(eval_str("(fiber-done-p f)", env), Ok(LispExp::t()));
    }

    // Exercise the shared binder through `call_callable`, separately from direct calls.

    #[test]
    fn funcall_and_apply_respect_optional_and_rest_params() {
        let script = "(defun f (a &optional b &rest c) (cons a (cons b c)))";
        assert_eq!(
            eval_ok(&format!("{script} (funcall 'f 1)")),
            LispExp::proper_list(vec![LispExp::number(1.0), LispExp::nil()])
        );
        assert_eq!(
            eval_ok(&format!("{script} (apply 'f 1 2 '(3 4))")),
            LispExp::proper_list(vec![
                LispExp::number(1.0),
                LispExp::number(2.0),
                LispExp::number(3.0),
                LispExp::number(4.0),
            ])
        );
    }

    // Printing

    /// Print readable Lisp data and opaque markers for runtime-only values.
    #[test]
    fn values_print_as_lisp_source() {
        let cases = [
            ("'(1 2 3)", "(1 2 3)"),
            ("(list)", "nil"),
            ("'()", "nil"),
            ("nil", "nil"),
            // A dotted pair, and a chain that only turns improper at the end.
            ("(cons 1 2)", "(1 . 2)"),
            ("(cons 1 (cons 2 3))", "(1 2 . 3)"),
            ("'(1 . 2)", "(1 . 2)"),
            // Nesting, vectors and strings inside a list.
            ("'(a (b c) [1 2] \"hi\")", "(a (b c) [1 2] \"hi\")"),
            ("'(nested (deep (deeper 1)))", "(nested (deep (deeper 1)))"),
            ("'(quote x)", "(quote x)"),
            // Integral numbers print without a fractional part.
            ("(+ 1 2)", "3"),
            ("(list 1.5 2.0 -3)", "(1.5 2 -3)"),
            // Values the reader cannot produce print in `#<...>` form.
            (
                "(lambda (a &optional b &rest c) a)",
                "#<lambda (a &optional b &rest c)>",
            ),
            ("(make-atom 5)", "#<atom>"),
        ];
        for (source, expected) in cases {
            assert_eq!(
                format!("{:?}", eval_ok(source)),
                expected,
                "printing the value of `{source}`"
            );
        }
    }

    #[test]
    fn make_string_rejects_unrepresentable_sizes() {
        for source in [
            r#"(make-string 100000000000000000000 "x")"#,
            r#"(make-string 100000000000000000000 "é")"#,
        ] {
            assert!(
                matches!(eval_err(source), EvalError::RuntimeMessage(_)),
                "input: {source}"
            );
        }
    }

    #[test]
    fn make_string_preserves_normal_behavior() {
        for (source, expected) in [
            (r#"(make-string 3 "x")"#, "xxx"),
            (r#"(make-string 3 "é")"#, "ééé"),
            (r#"(make-string 3 "ab")"#, "aaa"),
            (r#"(make-string 0 "x")"#, ""),
            (r#"(make-string -2 "x")"#, ""),
            (r#"(make-string 3 "")"#, ""),
        ] {
            assert_eq!(
                eval_ok(source),
                LispExp::string(expected.into()),
                "input: {source}"
            );
        }
    }

    #[test]
    fn explicit_and_abbreviated_quote_agree() {
        for literal in ["()", "(1 2)", "((1 2) (3 4))", "[1 (2 3)]", "{key (1 2)}"] {
            let source = format!("(equal '{literal} (quote {literal}))");

            assert_eq!(eval_ok(&source), LispExp::t(), "literal: {literal}");
        }
    }

    #[test]
    fn explicit_quote_returns_list_data() {
        assert_eq!(eval_ok("(car (quote (1 2)))"), LispExp::number(1.0));
        assert_eq!(eval_ok("(listp (quote (1 2)))"), LispExp::t());
        assert_eq!(eval_ok("(quote ())"), LispExp::nil());
    }

    #[test]
    fn quoted_literals_keep_their_identity() {
        for literal in ["(1 2)", "[1 2]", "{key 1}"] {
            let source = format!(
                "(defun quoted () (quote {literal}))
                 (eq (quoted) (quoted))"
            );

            assert_eq!(eval_ok(&source), LispExp::t(), "literal: {literal}");
        }
    }

    #[test]
    fn format_preserves_literal_backslashes() {
        for (source, expected) in [
            (
                r#"(format "\\b%s\\b" "word")"#,
                r"\bword\b",
            ),
            (
                r#"(format "C:\\Users\\%s" "me")"#,
                r"C:\Users\me",
            ),
            (
                r#"(format "\\n\\t\\r")"#,
                r"\n\t\r",
            ),
            (
                r#"(format "\\")"#,
                "\\",
            ),
            (
                r#"(format "\\%s" "x")"#,
                r"\x",
            ),
            (
                r#"(format "%s" "\\n\\b")"#,
                r"\n\b",
            ),
        ] {
            assert_eq!(
                eval_ok(source),
                LispExp::string(expected.into()),
                "source: {source}",
            );
        }
    }

    #[test]
    fn format_preserves_escapes_already_decoded_by_the_reader() {
        assert_eq!(
            eval_ok(r#"(format "a\nb\tc\rd")"#),
            LispExp::string("a\nb\tc\rd".into()),
        );
    }

    #[test]
    fn format_directives_still_work() {
        assert_eq!(
            eval_ok(r#"(format "%s %d %f %%" "x" -3.9 1.5)"#),
            LispExp::string("x -3 1.5 %".into()),
        );
    }

    #[test]
    fn format_still_rejects_missing_arguments() {
        assert!(matches!(
            eval_err(r#"(format "%s %d" "x")"#),
            EvalError::WrongNumberOfArguments {
                expected: 3,
                got: 2,
            }
        ));
    }

    #[test]
    fn number_to_string_handles_small_and_large_values() {
        for (literal, expected) in [
            ("0", "0"),
            ("-0.0", "0"),
            ("5", "5"),
            ("-5", "-5"),
            ("5.5", "5.5"),
            ("-5.5", "-5.5"),
            ("9007199254740991", "9007199254740991"),
            ("9007199254740992", "9007199254740992"),
            ("10000000000000000000", "10000000000000000000"),
            ("-10000000000000000000", "-10000000000000000000"),
        ] {
            let source = format!("(number-to-string {literal})");

            assert_eq!(
                eval_ok(&source),
                LispExp::string(expected.into()),
                "source: {source}",
            );
        }
    }

    #[test]
    fn number_to_string_round_trips_values_outside_i64_range() {
        for literal in [
            "18446744073709551616",
            "-18446744073709551616",
            "100000000000000000000",
        ] {
            let source = format!(
                "(= (string-to-number (number-to-string {literal}))
                    {literal})"
            );

            assert_eq!(
                eval_ok(&source),
                LispExp::t(),
                "source: {source}",
            );
        }
    }

    #[test]
    fn format_integer_truncates_without_saturating() {
        for (literal, expected) in [
            ("3.9", "3"),
            ("-3.9", "-3"),
            ("0.5", "0"),
            ("-0.5", "0"),
            ("-0.0", "0"),
            ("10000000000000000000", "10000000000000000000"),
            ("-10000000000000000000", "-10000000000000000000"),
            ("18446744073709551616", "18446744073709551616"),
            ("-18446744073709551616", "-18446744073709551616"),
        ] {
            let source = format!(r#"(format "%d" {literal})"#);

            assert_eq!(
                eval_ok(&source),
                LispExp::string(expected.into()),
                "source: {source}",
            );
        }
    }

    #[test]
    fn format_integer_rejects_non_finite_numbers() {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let env = env_with_primitives();

            // Inject through Rust so this test does not depend on the
            // reader's or string-to-number's policy for non-finite values.
            env.set_variable(
                "non-finite-value".into(),
                LispExp::number(value),
            );

            let error = eval_str(
                r#"(format "%d" non-finite-value)"#,
                env,
            )
            .expect_err("%d must reject non-finite numbers");

            match error {
                EvalError::WrongArgumentType {
                    expected,
                    got: LispExp::Number(number),
                } => {
                    assert_eq!(expected, "finite number for %d");

                    // NaN cannot be checked with assert_eq!.
                    if value.is_nan() {
                        assert!(number.is_nan());
                    } else {
                        assert_eq!(number, value);
                    }
                }
                other => panic!("unexpected error: {other:?}"),
            }
        }
    }
}
