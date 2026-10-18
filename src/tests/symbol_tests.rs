//! Symbol interning and properties.
//!
//! Check both observable equality and shared allocation identity so future
//! pointer-based comparisons remain valid.
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
        eval_str(source, env_with_primitives())
            .unwrap_or_else(|why| panic!("eval of `{source}` failed: {why:?}"))
    }

    /// The `Arc` behind a symbol, for the identity checks below.
    fn name_of(exp: &LispExp<()>) -> Arc<String> {
        match exp {
            LispExp::Symbol(name) => name.clone(),
            other => panic!("expected a symbol, got {other:?}"),
        }
    }

    /// Two symbols that are the same object, not merely the same spelling.
    fn same_object(a: &LispExp<()>, b: &LispExp<()>) -> bool {
        Arc::ptr_eq(&name_of(a), &name_of(b))
    }

    // Interning is total

    #[test]
    fn a_name_built_twice_is_one_object() {
        let first = LispExp::<()>::symbol("zzz-a-name".to_string());
        let second = LispExp::<()>::symbol("zzz-a-name".to_string());
        assert!(same_object(&first, &second));
    }

    #[test]
    fn different_names_are_different_objects() {
        // Distinct names must remain distinct allocations.
        let first = LispExp::<()>::symbol("zzz-one".to_string());
        let second = LispExp::<()>::symbol("zzz-two".to_string());
        assert!(!same_object(&first, &second));
        assert_ne!(first, second);
    }

    #[test]
    fn the_reader_interns_what_it_reads() {
        // The reader must use the same intern table as direct symbol construction.
        let parsed = eval_ok("'zzz-from-source");
        let built = LispExp::<()>::symbol("zzz-from-source".to_string());
        assert!(same_object(&parsed, &built));
    }

    #[test]
    fn two_occurrences_in_one_program_are_one_object() {
        let pair = eval_ok("(cons 'zzz-twice 'zzz-twice)");
        let LispExp::Cons(cell) = &pair else {
            panic!("expected a cons, got {pair:?}");
        };
        assert!(same_object(&cell.car, &cell.cdr));
    }

    #[test]
    fn nil_is_one_object_however_it_is_reached() {
        // The cached nil name must share the interned allocation.
        let constructed = LispExp::<()>::nil();
        let built = LispExp::<()>::symbol("nil".to_string());
        let parsed = eval_ok("'nil");
        let returned = eval_ok("(if nil 1)");

        assert!(
            same_object(&constructed, &built),
            "the constructor and the name"
        );
        assert!(
            same_object(&constructed, &parsed),
            "and what the reader makes"
        );
        assert!(
            same_object(&constructed, &returned),
            "and what eval hands back"
        );
    }

    #[test]
    fn t_is_one_object_however_it_is_reached() {
        let constructed = LispExp::<()>::t();
        let built = LispExp::<()>::symbol("t".to_string());
        let parsed = eval_ok("'t");
        assert!(same_object(&constructed, &built));
        assert!(same_object(&constructed, &parsed));
    }

    #[test]
    fn a_symbol_made_from_a_computed_name_is_the_same_object() {
        // Runtime interning and source literals must share the same symbol name.
        let computed = eval_ok("(intern (concat \"zzz-com\" \"puted\"))");
        let written = eval_ok("'zzz-computed");
        assert!(same_object(&computed, &written));
        assert_eq!(eval_ok("(eq (intern \"zzz-x\") 'zzz-x)"), LispExp::t());
    }

    // Observable equality.

    #[test]
    fn eq_holds_for_symbols_spelled_the_same() {
        assert_eq!(eval_ok("(eq 'alpha 'alpha)"), LispExp::t());
        assert_eq!(eval_ok("(eq 'alpha 'beta)"), LispExp::nil());
        assert_eq!(eval_ok("(eq nil nil)"), LispExp::t());
        assert_eq!(eval_ok("(eq t t)"), LispExp::t());
        assert_eq!(eval_ok("(eq nil t)"), LispExp::nil());
    }

    #[test]
    fn a_symbol_is_not_eq_to_the_string_of_its_name() {
        // A string and a symbol with the same text remain different value types.
        assert_eq!(eval_ok("(eq 'alpha \"alpha\")"), LispExp::nil());
        assert_eq!(eval_ok("(equal 'alpha \"alpha\")"), LispExp::nil());
    }

    #[test]
    fn nil_is_recognised_however_it_is_reached() {
        assert_eq!(eval_ok("(null nil)"), LispExp::t());
        assert_eq!(eval_ok("(null 'nil)"), LispExp::t());
        assert_eq!(eval_ok("(null (intern \"nil\"))"), LispExp::t());
        assert_eq!(eval_ok("(null t)"), LispExp::nil());
        assert!(LispExp::<()>::symbol("nil".to_string()).is_nil());
        assert!(!LispExp::<()>::symbol("nill".to_string()).is_nil());
    }

    #[test]
    fn a_symbol_still_knows_its_own_name() {
        assert_eq!(
            eval_ok("(symbol-name 'alpha)"),
            LispExp::string("alpha".to_string())
        );
    }

    // Properties

    #[test]
    fn a_property_comes_back() {
        assert_eq!(eval_ok("(put 'alpha 'colour 'red) (get 'alpha 'colour)"), {
            LispExp::symbol("red".to_string())
        });
    }

    #[test]
    fn putting_again_replaces() {
        assert_eq!(
            eval_ok("(put 'alpha 'colour 'red) (put 'alpha 'colour 'blue) (get 'alpha 'colour)"),
            LispExp::symbol("blue".to_string())
        );
    }

    #[test]
    fn put_answers_with_the_value_it_stored() {
        assert_eq!(eval_ok("(put 'alpha 'n 7)"), LispExp::number(7.0));
    }

    #[test]
    fn an_unset_property_is_nil() {
        assert_eq!(eval_ok("(get 'alpha 'never-set)"), LispExp::nil());
        assert_eq!(eval_ok("(get 'never-mentioned 'colour)"), LispExp::nil());
    }

    #[test]
    fn keys_do_not_collide_across_symbols() {
        assert_eq!(
            eval_ok("(put 'alpha 'colour 'red) (put 'beta 'colour 'blue) (get 'alpha 'colour)"),
            LispExp::symbol("red".to_string())
        );
    }

    #[test]
    fn keys_do_not_collide_within_a_symbol() {
        assert_eq!(
            eval_ok("(put 'alpha 'colour 'red) (put 'alpha 'size 3) (get 'alpha 'colour)"),
            LispExp::symbol("red".to_string())
        );
    }

    #[test]
    fn a_property_set_inside_a_let_is_still_there_outside_it() {
        // Properties written in a child scope must remain visible from the root.
        assert_eq!(
            eval_ok("(let ((ignored 1)) (put 'alpha 'colour 'red)) (get 'alpha 'colour)"),
            LispExp::symbol("red".to_string())
        );
    }

    #[test]
    fn a_property_set_inside_a_function_is_still_there_outside_it() {
        // A closure must reach the same root property table as its caller.
        assert_eq!(
            eval_ok(
                "(defun zzz-set () (put 'alpha 'colour 'green)) (zzz-set) (get 'alpha 'colour)"
            ),
            LispExp::symbol("green".to_string())
        );
    }

    #[test]
    fn a_property_is_read_the_same_from_inside_a_let() {
        assert_eq!(
            eval_ok("(put 'alpha 'colour 'red) (let ((ignored 1)) (get 'alpha 'colour))"),
            LispExp::symbol("red".to_string())
        );
    }

    #[test]
    fn a_name_may_be_written_as_a_symbol_or_a_string() {
        assert_eq!(
            eval_ok("(put \"alpha\" \"colour\" 'red) (get 'alpha 'colour)"),
            LispExp::symbol("red".to_string())
        );
    }

    #[test]
    fn a_property_may_hold_any_value() {
        assert_eq!(
            eval_ok("(put 'alpha 'words '(\"fn\" \"let\")) (length (get 'alpha 'words))"),
            LispExp::number(2.0)
        );
    }

    #[test]
    fn properties_are_not_variables() {
        // Symbol properties neither create bindings nor depend on variable shadowing.
        let env = env_with_primitives();
        eval_str("(setq alpha 1) (put 'alpha 'colour 'red)", env.clone()).expect("setup");
        assert_eq!(
            eval_str("(get 'alpha 'colour)", env.clone()).expect("read"),
            LispExp::symbol("red".to_string())
        );
        assert_eq!(
            eval_str("alpha", env).expect("the variable is untouched"),
            LispExp::number(1.0)
        );
    }

    #[test]
    fn put_and_get_check_their_arguments() {
        let env = env_with_primitives();
        assert!(eval_str("(put 1 'colour 'red)", env.clone()).is_err());
        assert!(eval_str("(get 1 'colour)", env.clone()).is_err());
        assert!(eval_str("(put 'alpha 'colour)", env.clone()).is_err());
        assert!(eval_str("(get 'alpha)", env).is_err());
    }
}
