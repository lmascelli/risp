#![cfg(test)]

use crate::{
    Env, EvalError, LispContext, LispExp, Parser, call_callable, eval,
    setup_base_env,
};
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const CASE_ENV: &str = "RISP_ALIAS_TEST_CASE";
const PROBE_OK: &str = "risp-alias-cycle-rejected";

// Unit-test names include their module path.
const PROBE_TEST: &str = "tests::function_aliases::alias_cycle_probe";

#[derive(Clone, Debug, PartialEq)]
struct Host;

impl LispContext for Host {}

fn environment() -> Arc<Env<Host>> {
    let env = Env::new_root();
    setup_base_env(env.clone());
    env
}

#[test]
fn alias_chain_reaches_its_target() {
    let env = environment();

    env.set_function("a".into(), LispExp::symbol("b".into()));
    env.set_function("b".into(), LispExp::symbol("identity".into()));

    let result = call_callable(
        &LispExp::symbol("a".into()),
        &[LispExp::number(42.0)],
        env,
        &Host,
    );

    assert_eq!(result, Ok(LispExp::number(42.0)));
}

#[test]
fn missing_alias_target_reports_its_name() {
    let env = environment();

    env.set_function("a".into(), LispExp::symbol("missing".into()));

    let result =
        call_callable(&LispExp::symbol("a".into()), &[], env, &Host);

    assert_eq!(
        result,
        Err(EvalError::UndefinedFunction("missing".into()))
    );
}

#[test]
fn self_referential_alias_returns_an_error() {
    assert_cycle_rejected("self");
}

#[test]
fn mutually_recursive_aliases_return_an_error() {
    assert_cycle_rejected("mutual");
}

// Ensure a failed assertion or timeout cannot leave the child running.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn assert_cycle_rejected(case: &str) {
    let executable =
        std::env::current_exe().expect("locate unit-test executable");

    let mut child = ChildGuard(
        Command::new(executable)
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
            .expect("start alias-cycle probe"),
    );

    let started = Instant::now();
    let timeout = Duration::from_secs(5);

    let status = loop {
        if let Some(status) =
            child.0.try_wait().expect("poll alias-cycle probe")
        {
            break status;
        }

        assert!(
            started.elapsed() < timeout,
            "alias case {case}: child exceeded the {timeout:?} timeout"
        );

        thread::sleep(Duration::from_millis(10));
    };

    let mut stdout = String::new();
    let mut stderr = String::new();

    child
        .0
        .stdout
        .take()
        .expect("capture probe stdout")
        .read_to_string(&mut stdout)
        .expect("read probe stdout");

    child
        .0
        .stderr
        .take()
        .expect("capture probe stderr")
        .read_to_string(&mut stderr)
        .expect("read probe stderr");

    assert!(
        status.success(),
        "alias case {case}: child failed ({status})\n\
         stdout:\n{stdout}\n\
         stderr:\n{stderr}"
    );

    // A successful exit alone is insufficient: an incorrect filter can
    // select zero tests and still return success.
    assert!(
        stdout.contains(PROBE_OK),
        "alias case {case}: probe did not confirm the expected error\n\
         expected test: {PROBE_TEST}\n\
         stdout:\n{stdout}\n\
         stderr:\n{stderr}"
    );
}

#[test]
#[ignore = "invoked only by the parent tests in a subprocess"]
fn alias_cycle_probe() {
    let case =
        std::env::var(CASE_ENV).expect("probe case supplied by parent");
    let env = environment();

    let result = match case.as_str() {
        "self" => {
            env.set_function("a".into(), LispExp::symbol("a".into()));

            call_callable(
                &LispExp::symbol("a".into()),
                &[],
                env,
                &Host,
            )
        }

        "mutual" => {
            env.set_function("a".into(), LispExp::symbol("b".into()));
            env.set_function("b".into(), LispExp::symbol("a".into()));

            let form = Parser::new("(funcall 'a 42)")
                .next::<Host>()
                .expect("parse mutual-alias call");

            eval(&form, env, &Host)
        }

        _ => panic!("unknown probe case: {case}"),
    };

    assert_eq!(
        result,
        Err(EvalError::UncorrectFunctionDefinition)
    );

    println!("{PROBE_OK}");
}