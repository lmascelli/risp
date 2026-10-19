//! Variable docstrings: literal storage, redefinition, lexical shadowing,
//! and lookup of documented, undocumented, and unbound symbols.
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
        let wrapped = format!("(progn {source})");
        let ast = Parser::new(&wrapped)
            .next()
            .expect("failed to parse test script");
        eval(&ast, env, &())
    }

    fn eval_in(source: &str, env: Arc<Env<()>>) -> LispExp<()> {
        eval_str(source, env).unwrap_or_else(|why| panic!("eval of `{source}` failed: {why:?}"))
    }

    fn eval_ok(source: &str) -> LispExp<()> {
        eval_in(source, env_with_primitives())
    }

    fn text(exp: &LispExp<()>) -> String {
        match exp {
            LispExp::String(text) => text.to_string(),
            other => panic!("expected a string, got {other:?}"),
        }
    }

    #[test]
    fn a_defvar_docstring_can_be_read_back() {
        let doc = eval_ok(
            r#"(defvar fill-column 70 "Where lines are wrapped.")
               (variable-doc 'fill-column)"#,
        );
        assert_eq!(text(&doc), "Where lines are wrapped.");
    }

    #[test]
    fn defconst_documents_the_same_way() {
        let doc = eval_ok(
            r#"(defconst pi-ish 3.14 "Close enough for a test.")
               (variable-doc 'pi-ish)"#,
        );
        assert_eq!(text(&doc), "Close enough for a test.");
    }

    #[test]
    fn a_docstring_is_taken_literally_and_never_evaluated() {
        // A docstring is literal; rejecting an expression must not evaluate its side effects.
        let env = env_with_primitives();
        let failed = eval_str(r#"(defvar x 1 (concat "a" "b"))"#, env.clone());
        assert!(
            matches!(failed, Err(EvalError::DefvarDocMustBeAString)),
            "a computed docstring should be refused, got {failed:?}"
        );
        // Invalid documentation must leave both the value and property untouched.
        assert_eq!(
            eval_in("(get 'x 'variable-documentation)", env.clone()),
            LispExp::nil()
        );
        assert!(
            matches!(eval_str("x", env), Err(EvalError::UnboundVariable(_))),
            "the refused form should have bound nothing"
        );
    }

    #[test]
    fn a_fourth_form_is_refused_rather_than_ignored() {
        let failed = eval_str(r#"(defvar x 1 "doc" surprise)"#, env_with_primitives());
        assert!(
            matches!(failed, Err(EvalError::WrongNumberOfArguments { .. })),
            "a fourth form should be refused, got {failed:?}"
        );
    }

    #[test]
    fn a_variable_can_be_documented_before_it_has_a_value() {
        // A symbol may keep documentation without a variable binding.
        let env = env_with_primitives();
        eval_in(
            r#"(defvar later nil "Set by whatever runs first.")"#,
            env.clone(),
        );
        assert_eq!(
            text(&eval_in("(variable-doc 'later)", env.clone())),
            "Set by whatever runs first."
        );
    }

    #[test]
    fn re_evaluating_a_defvar_updates_the_docstring_but_not_the_value() {
        // Reloading refreshes the documentation while preserving the user's value.
        let env = env_with_primitives();
        eval_in(
            r#"(defvar tab-width 8 "The old description.")"#,
            env.clone(),
        );
        eval_in("(setq tab-width 4)", env.clone());
        eval_in(
            r#"(defvar tab-width 8 "The new description.")"#,
            env.clone(),
        );
        assert_eq!(eval_in("tab-width", env.clone()), LispExp::number(4.0));
        assert_eq!(
            text(&eval_in("(variable-doc 'tab-width)", env)),
            "The new description."
        );
    }

    #[test]
    fn a_docstring_can_be_taken_away_by_leaving_it_out() {
        // An omitted docstring leaves the existing property unchanged.
        let env = env_with_primitives();
        eval_in(r#"(defvar kept 1 "Still true.")"#, env.clone());
        eval_in("(defvar kept 1)", env.clone());
        assert_eq!(text(&eval_in("(variable-doc 'kept)", env)), "Still true.");
    }

    #[test]
    fn a_let_shadows_the_value_and_not_the_documentation() {
        let env = env_with_primitives();
        eval_in(r#"(defvar depth 1 "How deep.")"#, env.clone());
        let inside = eval_in(
            "(let ((depth 99)) (list depth (variable-doc 'depth)))",
            env.clone(),
        );
        let LispExp::Cons(cell) = &inside else {
            panic!("expected a list, got {inside:?}");
        };
        assert_eq!(cell.car, LispExp::number(99.0));
        assert_eq!(text(&eval_in("(variable-doc 'depth)", env)), "How deep.");
    }

    #[test]
    fn a_docstring_written_inside_a_let_outlives_it() {
        // Documentation belongs to the root property table, not the child binding.
        let env = env_with_primitives();
        eval_in(
            r#"(let ((ignored 1)) (defvar escaped 2 "Made inside."))"#,
            env.clone(),
        );
        assert_eq!(
            text(&eval_in("(variable-doc 'escaped)", env)),
            "Made inside."
        );
    }

    #[test]
    fn a_bound_variable_with_no_docstring_says_so() {
        let env = env_with_primitives();
        eval_in("(setq plain 1)", env.clone());
        assert_eq!(
            text(&eval_in("(variable-doc 'plain)", env)),
            "Undocumented variable"
        );
    }

    #[test]
    fn a_name_that_is_neither_bound_nor_documented_reads_as_nil() {
        assert_eq!(eval_ok("(variable-doc 'nothing-at-all)"), LispExp::nil());
    }

    #[test]
    fn the_docstring_is_an_ordinary_property() {
        // The documentation accessor and generic property accessor must agree.
        let doc = eval_ok(
            r#"(defvar shared 1 "Under a property.")
               (get 'shared 'variable-documentation)"#,
        );
        assert_eq!(text(&doc), "Under a property.");
    }

    #[test]
    fn documenting_a_variable_does_not_document_the_function_of_the_same_name() {
        // Variable and function documentation occupy separate namespaces.
        let env = env_with_primitives();
        eval_in(r#"(defvar both 1 "The variable.")"#, env.clone());
        eval_in(r#"(defun both () "The function." 1)"#, env.clone());
        assert_eq!(
            text(&eval_in("(variable-doc 'both)", env.clone())),
            "The variable."
        );
        assert_eq!(text(&eval_in("(function-doc 'both)", env)), "The function.");
    }

    #[test]
    fn variable_doc_wants_a_symbol() {
        let failed = eval_str(r#"(variable-doc "fill-column")"#, env_with_primitives());
        assert!(
            matches!(failed, Err(EvalError::WrongArgumentType { .. })),
            "a string should be refused, got {failed:?}"
        );
    }
}
