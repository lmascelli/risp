#![cfg(test)]

use crate::utils::condition_matches;
use crate::{
    Env, EvalError, LispContext, LispExp, Parser, eval, setup_base_env,
};
use std::{
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
fn deep_backquote_does_not_overflow_stack() {
    assert_deep_walk("backquote");
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
    let mut template = LispExp::number(42.0);
    for index in 0..DEPTH {
        template = if index % 2 == 0 {
            LispExp::form(vec![template])
        } else {
            LispExp::vec(vec![template])
        };
    }

    let code = LispExp::form(vec![symbol("backquote"), template]);
    let result = eval(&code, Env::new_root(), &Host)
        .expect("expand deep backquote");
    let mut cursor = &result;

    for index in (0..DEPTH).rev() {
        cursor = match (index % 2, cursor) {
            (0, LispExp::Cons(cell)) => {
                assert!(cell.cdr.is_nil());
                &cell.car
            }
            (1, LispExp::Vector(items)) => {
                assert_eq!(items.len(), 1);
                &items[0]
            }
            _ => panic!("unexpected container at level {index}"),
        };
    }

    assert_eq!(cursor, &LispExp::number(42.0));
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