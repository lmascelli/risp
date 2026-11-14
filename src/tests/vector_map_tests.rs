//! Vector and map primitives: construction, reads, copying updates, key
//! normalization, sorted traversal, errors, and fuel charging.
#[cfg(test)]
mod tests {
    use crate::{Env, EvalError, LispContext, LispExp, Parser, eval, setup_base_env};
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

    fn eval_err(source: &str) -> EvalError<()> {
        eval_str(source).expect_err(&format!("expected `{source}` to fail"))
    }

    fn printed(source: &str) -> String {
        format!("{:?}", eval_ok(source))
    }

    fn is_true(source: &str) -> bool {
        eval_ok(source).is_truthy()
    }

    // Vectors

    #[test]
    fn vector_and_make_vector_build_vectors() {
        assert_eq!(printed("(vector 1 \"a\" 'b)"), r#"[1 "a" b]"#);
        assert_eq!(printed("(vector)"), "[]");
        assert_eq!(printed("(make-vector 3 'x)"), "[x x x]");
        assert_eq!(printed("(make-vector 0 'x)"), "[]");
        assert_eq!(printed("(make-vector -2 'x)"), "[]");
        assert!(is_true("(equal (vector 1 2) [1 2])"));
    }

    #[test]
    fn aref_reads_an_element_by_index() {
        assert_eq!(printed("(aref '[a b c] 0)"), "a");
        assert_eq!(printed("(aref '[a b c] 2)"), "c");
    }

    #[test]
    fn aref_signals_args_out_of_range_with_the_vector_and_index() {
        for index in ["3", "-1", "1.5"] {
            let source = format!(
                "(condition-case err (aref '[a b c] {index}) (args-out-of-range (cdr err)))"
            );
            assert_eq!(printed(&source), format!("([a b c] {index})"));
        }
    }

    #[test]
    fn aref_rejects_a_non_vector_and_a_non_number_index() {
        assert!(matches!(
            eval_err("(aref '(a b) 0)"),
            EvalError::WrongArgumentType { .. }
        ));
        assert!(matches!(
            eval_err("(aref '[a b] 'x)"),
            EvalError::WrongArgumentType { .. }
        ));
    }

    #[test]
    fn vector_set_returns_a_copy_and_leaves_the_original() {
        assert_eq!(
            printed("(setq v '[a b c]) (list (vector-set v 1 'x) v)"),
            "([a x c] [a b c])"
        );
        assert!(matches!(
            eval_err("(vector-set '[a] 1 'x)"),
            EvalError::Signal { .. }
        ));
    }

    #[test]
    fn mapcar_and_mapc_accept_vectors() {
        assert_eq!(printed("(mapcar '1+ [1 2 3])"), "(2 3 4)");
        assert_eq!(printed("(mapcar '1+ [])"), "nil");
        assert_eq!(
            printed("(let ((n 0)) (list (mapc (lambda (x) (setq n (+ n x))) [1 2 3]) n))"),
            "([1 2 3] 6)"
        );
    }

    // Maps

    #[test]
    fn make_map_accepts_symbol_and_string_keys_like_the_reader() {
        assert!(is_true("(equal (make-map 'a 1 \"b\" 2) {a 1 b 2})"));
        assert!(is_true("(equal (make-map \"a\" 1) (make-map 'a 1))"));
        assert_eq!(printed("(make-map)"), "{}");
        assert_eq!(printed("(make-map 'a 1 'a 2)"), "{a 2}");
    }

    #[test]
    fn make_map_rejects_an_odd_argument_count_and_bad_keys() {
        assert!(matches!(
            eval_err("(make-map 'a 1 'b)"),
            EvalError::WrongNumberOfArguments {
                expected: 4,
                got: 3
            }
        ));
        assert!(matches!(
            eval_err("(make-map 1 'x)"),
            EvalError::WrongArgumentType { .. }
        ));
    }

    #[test]
    fn mapp_recognizes_only_maps() {
        assert!(is_true("(mapp {a 1})"));
        assert!(is_true("(mapp (make-map))"));
        assert!(!is_true("(mapp '((a . 1)))"));
        assert!(!is_true("(mapp '[a 1])"));
    }

    #[test]
    fn map_elt_reads_with_an_optional_default() {
        assert_eq!(printed("(map-elt {a 1} 'a)"), "1");
        assert_eq!(printed("(map-elt {a 1} \"a\")"), "1");
        assert_eq!(printed("(map-elt {a 1} 'b)"), "nil");
        assert_eq!(printed("(map-elt {a 1} 'b 'none)"), "none");
        assert!(matches!(
            eval_err("(map-elt {a 1})"),
            EvalError::WrongNumberOfArguments { .. }
        ));
        assert!(matches!(
            eval_err("(map-elt '((a . 1)) 'a)"),
            EvalError::WrongArgumentType { .. }
        ));
    }

    #[test]
    fn map_contains_key_sees_an_entry_holding_nil() {
        assert!(is_true("(map-contains-key {a nil} 'a)"));
        assert!(!is_true("(map-contains-key {a nil} 'b)"));
    }

    #[test]
    fn map_insert_and_map_delete_return_copies() {
        assert_eq!(
            printed("(setq m {a 1}) (list (map-insert m 'b 2) (map-insert m 'a 9) m)"),
            "({a 1 b 2} {a 9} {a 1})"
        );
        assert_eq!(
            printed("(setq m {a 1 b 2}) (list (map-delete m 'a) (map-delete m 'z) m)"),
            "({b 2} {a 1 b 2} {a 1 b 2})"
        );
    }

    #[test]
    fn keys_values_and_pairs_follow_sorted_key_order() {
        let map = "{c 3 a 1 b 2}";
        assert_eq!(printed(&format!("(map-keys {map})")), r#"("a" "b" "c")"#);
        assert_eq!(printed(&format!("(map-values {map})")), "(1 2 3)");
        assert_eq!(
            printed(&format!("(map-pairs {map})")),
            r#"(("a" . 1) ("b" . 2) ("c" . 3))"#
        );
        assert_eq!(printed("(map-keys {})"), "nil");
    }

    #[test]
    fn map_length_counts_entries() {
        assert_eq!(printed("(map-length {})"), "0");
        assert_eq!(printed("(map-length {a 1 b 2})"), "2");
    }

    #[test]
    fn map_do_and_map_apply_call_with_string_keys_in_order() {
        assert_eq!(
            printed(
                "(let ((seen nil))
                   (list (map-do (lambda (k v) (setq seen (cons (cons k v) seen))) {b 2 a 1})
                         seen))"
            ),
            r#"(nil (("b" . 2) ("a" . 1)))"#
        );
        assert_eq!(
            printed("(map-apply (lambda (k v) (concat k \"=\" (number-to-string v))) {b 2 a 1})"),
            r#"("a=1" "b=2")"#
        );
        assert_eq!(printed("(map-apply 'list {})"), "nil");
    }

    #[test]
    fn a_map_prints_its_keys_in_sorted_order() {
        assert_eq!(printed("{z 1 a [2] m \"x\"}"), r#"{a [2] m "x" z 1}"#);
    }

    #[test]
    fn every_new_docstring_starts_with_its_signature() {
        for name in [
            "vector",
            "make-vector",
            "aref",
            "vector-set",
            "make-map",
            "mapp",
            "map-elt",
            "map-contains-key",
            "map-insert",
            "map-delete",
            "map-keys",
            "map-values",
            "map-pairs",
            "map-length",
            "map-do",
            "map-apply",
        ] {
            let LispExp::String(doc) = eval_ok(&format!("(function-doc '{name})")) else {
                panic!("{name} has no documentation string");
            };
            assert!(
                doc.starts_with(&format!("({name}")) && doc.contains("): "),
                "{name}: {doc}"
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

    fn eval_with_budget(
        source: &str,
        budget: u32,
    ) -> (Result<LispExp<Budget>, EvalError<Budget>>, u32) {
        let env = Env::new_root();
        setup_base_env(env.clone());
        let ast = Parser::new(source).next().expect("failed to parse");
        let left = Arc::new(AtomicU32::new(budget));
        let result = eval(&ast, env, &Budget(left.clone()));
        (result, budget - left.load(Ordering::Relaxed))
    }

    #[test]
    fn make_vector_charges_its_length_before_allocating() {
        let (result, _) = eval_with_budget("(make-vector 4000000000 0)", 1_000);
        assert!(matches!(result, Err(EvalError::OutOfFuel)));
        let (result, spent) = eval_with_budget("(make-vector 10 0)", 1_000);
        assert!(result.is_ok());
        assert!(spent >= 10, "spent {spent}");
    }

    #[test]
    fn map_traversals_charge_per_entry() {
        let entries = (0..50)
            .map(|i| format!("\"k{i}\" {i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let (_, building) = eval_with_budget(&format!("(map-length (make-map {entries}))"), 10_000);
        for traversal in ["map-keys", "map-values", "map-pairs"] {
            let source = format!("({traversal} (make-map {entries}))");
            let (result, spent) = eval_with_budget(&source, 10_000);
            assert!(result.is_ok());
            assert!(spent >= building + 50, "{traversal}: {spent} vs {building}");
        }
    }
}
