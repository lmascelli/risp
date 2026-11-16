#![cfg(test)]

use crate::utils::condition_matches;
use crate::{
    Env, EvalError, LispContext, LispExp, Parser, eval, setup_base_env,
};
use std::{
    collections::HashMap,
    io::Read,
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const PROBE_TEST: &str = "tests::stack_safety_tests::deep_walk_probe";
const CASE_ENV: &str = "RISP_DEEP_WALK_CASE";
const PROBE_OK: &str = "risp-deep-walk-ok";
const DEPTH: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
struct Host;

impl LispContext for Host {}

fn symbol(name: &str) -> LispExp<Host> {
    LispExp::symbol(name.into())
}

fn environment() -> Arc<Env<Host>> {
    let env = Env::new_root();
    setup_base_env(env.clone());
    env
}

fn eval_source(
    source: &str,
    env: Arc<Env<Host>>,
) -> Result<LispExp<Host>, EvalError<Host>> {
    let form = Parser::new(source)
        .next::<Host>()
        .expect("parse test expression");
    eval(&form, env, &Host)
}

#[test]
fn conditions_match_nested_symbols_and_wildcards() {
    let conditions = LispExp::form(vec![
        symbol("wrong-type-argument"),
        LispExp::form(vec![symbol("unbound-variable")]),
    ]);

    assert!(condition_matches(&conditions, &symbol("unbound-variable")));
    assert!(!condition_matches(&conditions, &symbol("undefined-function")));
    assert!(condition_matches(&symbol("t"), &symbol("anything")));
    assert!(condition_matches(&symbol("error"), &symbol("anything")));
    assert!(!condition_matches(
        &LispExp::form(vec![]),
        &symbol("anything"),
    ));
}

#[test]
fn backquote_preserves_expansion_order() {
    let env = environment();
    env.set_variable("n".into(), LispExp::number(0.0));

    let result = eval_source(
        "(backquote
           ((unquote (setq n (+ n 1)))
            (unquote-splicing
              (list (setq n (+ n 1)) (setq n (+ n 1))))
            (unquote-splicing nil)
            ((unquote (setq n (+ n 1))))
            [(unquote (setq n (+ n 1)))]))",
        env.clone(),
    );

    assert_eq!(
        result,
        Ok(LispExp::proper_list(vec![
            LispExp::number(1.0),
            LispExp::number(2.0),
            LispExp::number(3.0),
            LispExp::proper_list(vec![LispExp::number(4.0)]),
            LispExp::vec(vec![LispExp::number(5.0)]),
        ])),
    );
    assert_eq!(env.get_variable("n"), Some(LispExp::number(5.0)));
}

#[test]
fn backquote_two_levels_delay_unquotes_and_splices() {
    let env = environment();
    env.set_variable("n".into(), LispExp::number(0.0));
    let first = eval_source(
        "``(,(setq n (+ n 10)) ,,(setq n (+ n 1))
            ,(list ,@(list (setq n (+ n 1))))
            ,@(list (setq n (+ n 100))))",
        env.clone(),
    ).expect("expand outer backquote");
    assert_eq!(env.get_variable("n"), Some(LispExp::number(2.0)));
    assert_eq!(
        Ok(first.clone()),
        eval_source(
            "'`(,(setq n (+ n 10)) ,1 ,(list 2)
                 ,@(list (setq n (+ n 100))))",
            env.clone(),
        ),
    );
    assert_eq!(
        eval(&first, env.clone(), &Host),
        eval_source("'(12 1 (2) 112)", env.clone()),
    );
    assert_eq!(env.get_variable("n"), Some(LispExp::number(112.0)));
}

#[test]
fn backquote_three_levels_expand_at_the_matching_depth() {
    let env = environment();
    env.set_variable("n".into(), LispExp::number(0.0));
    let mut result = Parser::new(
        "```(,(setq n (+ n 100)) ,,(setq n (+ n 10))
             ,,,(setq n (+ n 1)) ,@(list (setq n (+ n 1000))))",
    ).next::<Host>().expect("parse nested backquotes");
    for (expected_n, expected) in [
        (1.0, "'``(,(setq n (+ n 100)) ,,(setq n (+ n 10))
                    ,,1 ,@(list (setq n (+ n 1000))))"),
        (11.0, "'`(,(setq n (+ n 100)) ,11 ,1
                    ,@(list (setq n (+ n 1000))))"),
        (1111.0, "'(111 11 1 1111)"),
    ] {
        result = eval(&result, env.clone(), &Host).expect("expand next backquote");
        assert_eq!(env.get_variable("n"), Some(LispExp::number(expected_n)));
        assert_eq!(Ok(result.clone()), eval_source(expected, env.clone()));
    }
}

#[test]
fn backquote_depth_reaches_vectors_maps_and_dotted_tails() {
    let env = environment();
    env.set_variable("n".into(), LispExp::number(0.0));
    assert_eq!(
        eval_source(
            "`(,(setq n (+ n 1))
                `([,(setq n 99) ,,(setq n (+ n 1))]
                  {a ,(setq n 99) z ,,(setq n (+ n 1))}
                  (,(setq n 99) . ,,(setq n (+ n 1))))
                ,(setq n (+ n 1)))",
            env.clone(),
        ),
        eval_source(
            "'(1 `([,(setq n 99) ,2] {a ,(setq n 99) z ,3}
                    (,(setq n 99) . ,4)) 5)",
            env.clone(),
        ),
    );
    assert_eq!(env.get_variable("n"), Some(LispExp::number(5.0)));
}

#[test]
fn spliced_values_are_not_expanded_again() {
    let env = environment();
    let item = LispExp::form(vec![
        symbol("unquote"),
        symbol("must-not-be-evaluated"),
    ]);
    env.set_variable(
        "items".into(),
        LispExp::proper_list(vec![item.clone()]),
    );

    assert_eq!(
        eval_source(
            "(backquote (head (unquote-splicing items) tail))",
            env,
        ),
        Ok(LispExp::proper_list(vec![
            symbol("head"),
            item,
            symbol("tail"),
        ])),
    );
}

#[test]
fn splices_accept_syntax_values_returned_by_native_functions() {
    let env = environment();
    env.set_function(
        "host-items".into(),
        LispExp::native(
            |_, _, _| {
                Ok(LispExp::form(vec![
                    LispExp::number(2.0),
                    LispExp::number(3.0),
                ]))
            },
            None,
        ),
    );

    assert_eq!(
        eval_source(
            "(backquote (1 (unquote-splicing (host-items)) 4))",
            env,
        ),
        Ok(LispExp::proper_list(vec![
            LispExp::number(1.0),
            LispExp::number(2.0),
            LispExp::number(3.0),
            LispExp::number(4.0),
        ])),
    );
}

#[test]
fn invalid_splice_stops_before_later_unquotes() {
    let env = environment();
    env.set_variable("n".into(), LispExp::number(0.0));

    let result = eval_source(
        "(backquote
           ((unquote (setq n 1))
            (unquote-splicing 42)
            (unquote (setq n 2))))",
        env.clone(),
    );

    assert_eq!(
        result,
        Err(EvalError::WrongArgumentType {
            expected: "List".into(),
            got: LispExp::number(42.0),
        }),
    );
    assert_eq!(env.get_variable("n"), Some(LispExp::number(1.0)));
}

#[test]
fn backquote_expands_dotted_tails_and_map_values() {
    let env = environment();
    env.set_variable("b".into(), LispExp::number(2.0));
    for (source, expected) in [
        ("`(a . ,b)", "'(a . 2)"),
        ("`((a . ,b) {value ,b} {})", "'((a . 2) {value 2} {})"),
        ("`(,@nil . ,b)", "2"),
        ("`(,@'(3 4) . ,b)", "'(3 4 . 2)"),
        ("`(,@nil . nil)", "nil"),
    ] {
        assert_eq!(
            eval_source(source, env.clone()),
            eval_source(expected, env.clone()),
            "source: {source}",
        );
    }
}

#[test]
fn backquote_orders_cons_splices_tails_and_map_values() {
    let env = environment();
    env.set_variable("n".into(), LispExp::number(0.0));
    let result = eval_source(
        "`((,(setq n (+ n 1)) ,@(list (setq n (+ n 1)))
             . {z ,(setq n (+ n 1)) a ,(setq n (+ n 1))})
           ,(setq n (+ n 1)))",
        env.clone(),
    );
    assert_eq!(
        result,
        eval_source("'((1 2 . {a 3 z 4}) 5)", env.clone()),
    );
    assert_eq!(env.get_variable("n"), Some(LispExp::number(5.0)));
}

#[test]
fn backquote_preserves_cons_data_and_unquoted_values() {
    let env = environment();
    let data = LispExp::proper_list(vec![
        symbol("unquote"), symbol("must-not-be-evaluated"),
    ]);
    let syntax = LispExp::form(vec![
        symbol("unquote-splicing"), symbol("must-not-be-evaluated"),
    ]);
    env.set_variable("data".into(), data.clone());
    env.set_variable("syntax".into(), syntax.clone());
    assert_eq!(
        eval_source("`(,data {value ,syntax} . ,syntax)", env.clone()),
        Ok(LispExp::improper_list(
            vec![data.clone(), LispExp::map(HashMap::from([
                ("value".into(), syntax.clone()),
            ]))],
            syntax,
        )),
    );
    let code = LispExp::form(vec![symbol("backquote"), data.clone()]);
    assert_eq!(eval(&code, env, &Host), Ok(data));
}

#[test]
fn backquote_rejects_misplaced_splices_before_later_effects() {
    for (template, expected_n) in [
        (",@(setq n 99)", 0.0),
        ("(,(setq n 1) (a . ,@(setq n 99)) ,(setq n 2))", 1.0),
        ("(,(setq n 1) [,@(setq n 99)] ,(setq n 2))", 1.0),
        ("(,(setq n 1) {a ,@(setq n 99) z ,(setq n 2)})", 1.0),
    ] {
        let env = environment();
        env.set_variable("n".into(), LispExp::number(0.0));
        assert_eq!(
            eval_source(&format!("`{template}"), env.clone()),
            Err(EvalError::RuntimeMessage(
                "unquote-splicing is only allowed as a list element".into(),
            )),
            "template: {template}",
        );
        assert_eq!(env.get_variable("n"), Some(LispExp::number(expected_n)));
    }
}

#[test]
fn backquote_rejects_improper_splice_values_without_losing_the_tail() {
    let env = environment();
    env.set_variable("n".into(), LispExp::number(0.0));
    assert_eq!(
        eval_source(
            "`(,(setq n 1) ,@'(2 . 3) ,(setq n 2))",
            env.clone(),
        ),
        Err(EvalError::WrongArgumentType {
            expected: "List".into(),
            got: LispExp::cons(LispExp::number(2.0), LispExp::number(3.0)),
        }),
    );
    assert_eq!(env.get_variable("n"), Some(LispExp::number(1.0)));
}

#[test]
fn deep_backquote_does_not_overflow_stack() {
    assert_deep_walk("backquote");
}

#[test]
fn deep_nested_backquote_does_not_overflow_stack() {
    assert_deep_walk("nested-backquote");
}

#[test]
fn deep_condition_matching_does_not_overflow_stack() {
    assert_deep_walk("conditions");
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn assert_deep_walk(case: &str) {
    let mut child = ChildGuard(
        Command::new(
            std::env::current_exe().expect("locate unit-test executable"),
        )
        .args([
            "--exact",
            PROBE_TEST,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CASE_ENV, case)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start deep-walk probe"),
    );

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("poll deep-walk probe") {
            break status;
        }

        assert!(
            started.elapsed() < Duration::from_secs(10),
            "case {case}: child exceeded the timeout"
        );
        thread::sleep(Duration::from_millis(10));
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    child.0.stdout.take().unwrap()
        .read_to_string(&mut stdout).unwrap();
    child.0.stderr.take().unwrap()
        .read_to_string(&mut stderr).unwrap();

    assert!(
        status.success(),
        "case {case}: child failed ({status})\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains(PROBE_OK),
        "case {case}: probe did not confirm success\n{stdout}\n{stderr}"
    );
}

fn check_deep_backquote() {
    let mut template = LispExp::form(vec![symbol("unquote"), LispExp::number(42.0)]);
    for index in 0..DEPTH {
        template = match index % 4 {
            0 => LispExp::form(vec![template]),
            1 => LispExp::vec(vec![template]),
            2 => LispExp::cons(template, LispExp::number(0.0)),
            _ => LispExp::map(HashMap::from([("value".into(), template)])),
        };
    }

    let code = LispExp::form(vec![symbol("backquote"), template]);
    let result = eval(&code, Env::new_root(), &Host)
        .expect("expand deep backquote");
    let mut cursor = &result;

    for index in (0..DEPTH).rev() {
        cursor = match (index % 4, cursor) {
            (0, LispExp::Cons(cell)) => {
                assert!(cell.cdr.is_nil());
                &cell.car
            }
            (1, LispExp::Vector(items)) => {
                assert_eq!(items.len(), 1);
                &items[0]
            }
            (2, LispExp::Cons(cell)) => {
                assert_eq!(cell.cdr, LispExp::number(0.0));
                &cell.car
            }
            (3, LispExp::Map(items)) => {
                assert_eq!(items.len(), 1);
                &items["value"]
            }
            _ => panic!("unexpected container at level {index}"),
        };
    }

    assert_eq!(cursor, &LispExp::number(42.0));
}

fn check_deep_nested_backquote() {
    let mut template = LispExp::form(vec![
        symbol("unquote"), symbol("must-not-be-evaluated"),
    ]);
    for _ in 0..DEPTH {
        template = LispExp::form(vec![symbol("backquote"), template]);
    }
    let code = LispExp::form(vec![symbol("backquote"), template]);
    let result = eval(&code, Env::new_root(), &Host).expect("expand nested backquotes");
    let mut cursor = &result;
    for _ in 0..DEPTH {
        let LispExp::Cons(head) = cursor else { panic!("expected backquote") };
        assert_eq!(head.car, symbol("backquote"));
        let LispExp::Cons(operand) = &head.cdr else { panic!("expected operand") };
        assert!(operand.cdr.is_nil());
        cursor = &operand.car;
    }
    assert_eq!(cursor, &LispExp::proper_list(vec![
        symbol("unquote"), symbol("must-not-be-evaluated"),
    ]));
}

fn check_deep_conditions() {
    let mut conditions = symbol("unbound-variable");
    for _ in 0..DEPTH {
        conditions = LispExp::form(vec![conditions]);
    }

    assert!(condition_matches(&conditions, &symbol("unbound-variable")));
    assert!(!condition_matches(&conditions, &symbol("undefined-function")));

    let handler = LispExp::form(vec![conditions, LispExp::number(42.0)]);
    let code = LispExp::form(vec![
        symbol("condition-case"),
        LispExp::nil(),
        symbol("missing"),
        handler,
    ]);

    assert_eq!(
        eval(&code, Env::new_root(), &Host),
        Ok(LispExp::number(42.0)),
    );
}

#[test]
#[ignore = "invoked only by the parent tests in a subprocess"]
fn deep_walk_probe() {
    let case = std::env::var(CASE_ENV).expect("probe case supplied by parent");
    let check: fn() = match case.as_str() {
        "backquote" => check_deep_backquote,
        "nested-backquote" => check_deep_nested_backquote,
        "conditions" => check_deep_conditions,
        _ => panic!("unknown probe case: {case}"),
    };

    thread::Builder::new()
        .name(format!("int4-{case}"))
        .stack_size(2 * 1024 * 1024)
        .spawn(check)
        .expect("create probe thread")
        .join()
        .expect("probe thread panicked");

    println!("{PROBE_OK}");
}