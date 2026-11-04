#![cfg(test)]

use crate::{
    Env, EvalError, LispContext, LispExp, eval, form_to_data, setup_base_env,
};
use crate::utils::data_to_form;
use std::{
    collections::HashMap,
    io::Read,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const PROBE_TEST: &str = "tests::syntax_conversion::deep_conversion_probe";
const CASE_ENV: &str = "RISP_CONVERSION_TEST_CASE";
const PROBE_OK: &str = "risp-deep-conversion-ok";
const DEEP_NESTING: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
struct Host;

impl LispContext for Host {}

fn symbol(name: &str) -> LispExp<Host> {
    LispExp::symbol(name.into())
}

#[test]
fn data_to_form_preserves_order_and_vector_structure() {
    let data = LispExp::vec(vec![
        LispExp::proper_list(vec![
            symbol("+"),
            LispExp::number(20.0),
            LispExp::proper_list(vec![
                symbol("identity"),
                LispExp::number(22.0),
            ]),
        ]),
        LispExp::vec(vec![]),
    ]);

    let expected = LispExp::vec(vec![
        LispExp::form(vec![
            symbol("+"),
            LispExp::number(20.0),
            LispExp::form(vec![
                symbol("identity"),
                LispExp::number(22.0),
            ]),
        ]),
        LispExp::vec(vec![]),
    ]);

    assert_eq!(data_to_form(&data), Ok(expected));
}

#[test]
fn data_to_form_preserves_quoted_improper_data() {
    let payload = LispExp::improper_list(
        vec![symbol("a")],
        LispExp::number(42.0),
    );
    let data = LispExp::proper_list(vec![symbol("quote"), payload.clone()]);

    assert_eq!(
        data_to_form(&data),
        Ok(LispExp::form(vec![symbol("quote"), payload])),
    );
}

#[test]
fn data_to_form_rejects_improper_code_lists() {
    let data = LispExp::improper_list(
        vec![symbol("identity")],
        LispExp::number(42.0),
    );

    assert_eq!(
        data_to_form(&data),
        Err(EvalError::UnvalidFunctionCall),
    );
    assert_eq!(
        data_to_form(&LispExp::vec(vec![data])),
        Err(EvalError::UnvalidFunctionCall),
    );
}

#[test]
fn form_to_data_visits_vectors_and_maps() {
    let syntax = LispExp::<Host>::map(HashMap::from([
        (
            "code".into(),
            LispExp::vec(vec![LispExp::form(vec![
                symbol("+"),
                LispExp::number(20.0),
                LispExp::number(22.0),
            ])]),
        ),
        ("empty".into(), LispExp::form(vec![])),
    ]));

    let expected = LispExp::map(HashMap::from([
        (
            "code".into(),
            LispExp::vec(vec![LispExp::proper_list(vec![
                symbol("+"),
                LispExp::number(20.0),
                LispExp::number(22.0),
            ])]),
        ),
        ("empty".into(), LispExp::nil()),
    ]));

    assert_eq!(form_to_data(&syntax), expected);
}

#[test]
fn conversions_preserve_containers_outside_their_scope() {
    let data = LispExp::proper_list(vec![LispExp::form(vec![
        LispExp::<Host>::number(42.0),
    ])]);
    assert_eq!(form_to_data(&data), data);

    let syntax = LispExp::form(vec![data.clone()]);
    assert_eq!(data_to_form(&syntax), Ok(syntax.clone()));

    let map = LispExp::map(HashMap::from([("data".into(), data)]));
    assert_eq!(data_to_form(&map), Ok(map.clone()));
}

#[test]
fn eval_of_deep_runtime_code_returns_a_depth_error() {
    assert_deep_conversion("data");
}

#[test]
fn deeply_nested_forms_can_be_converted_to_data() {
    assert_deep_conversion("form");
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

fn assert_deep_conversion(case: &str) {
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
        .expect("start deep-conversion probe"),
    );

    let started = Instant::now();
    let status = loop {
        if let Some(status) =
            child.0.try_wait().expect("poll deep-conversion probe")
        {
            break status;
        }

        assert!(
            started.elapsed() < Duration::from_secs(10),
            "conversion case {case}: child exceeded the timeout"
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
        "conversion case {case}: child failed ({status})\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains(PROBE_OK),
        "conversion case {case}: probe did not run successfully\n{stdout}\n{stderr}"
    );
}

#[test]
#[ignore = "invoked only by the parent tests in a subprocess"]
fn deep_conversion_probe() {
    let case = std::env::var(CASE_ENV).expect("probe case supplied by parent");

    match case.as_str() {
        "data" => {
            let mut data = LispExp::number(0.0);
            for _ in 0..DEEP_NESTING {
                data = LispExp::proper_list(vec![symbol("identity"), data]);
            }

            let env = Env::new_root();
            setup_base_env(env.clone());

            assert_eq!(
                eval(&data, env, &Host),
                Err(EvalError::EvalDepthExceeded),
            );
        }

        "form" => {
            let mut syntax = LispExp::<Host>::number(42.0);
            for index in 0..DEEP_NESTING {
                syntax = match index % 3 {
                    0 => LispExp::form(vec![syntax]),
                    1 => LispExp::vec(vec![syntax]),
                    _ => LispExp::map(HashMap::from([
                        ("child".into(), syntax),
                    ])),
                };
            }

            let data = form_to_data(&syntax);
            let mut cursor = &data;

            for index in (0..DEEP_NESTING).rev() {
                cursor = match (index % 3, cursor) {
                    (0, LispExp::Cons(cell)) => {
                        assert!(cell.cdr.is_nil());
                        &cell.car
                    }
                    (1, LispExp::Vector(items)) => {
                        assert_eq!(items.len(), 1);
                        &items[0]
                    }
                    (2, LispExp::Map(items)) => {
                        assert_eq!(items.len(), 1);
                        items.get("child").expect("preserve map key")
                    }
                    _ => panic!("unexpected container at level {index}"),
                };
            }

            assert_eq!(cursor, &LispExp::number(42.0));
        }

        _ => panic!("unknown probe case: {case}"),
    }

    println!("{PROBE_OK}");
}