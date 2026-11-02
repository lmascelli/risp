use crate::{
    call_callable, eval, setup_base_env, Env, EvalError,
    LispContext, LispExp, Parser,
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

// Terminate and reap the child if a check or timeout fails.
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
    let mut child = ChildGuard(
        Command::new(
            std::env::current_exe().expect("locate test executable"),
        )
        .args([
            "--exact",
            "alias_cycle_probe",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CASE_ENV, case)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start alias probe"),
    );

    let started = Instant::now();
    let status = loop {
        if let Some(status) =
            child.0.try_wait().expect("poll alias probe")
        {
            break status;
        }

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "alias case {case}: child exceeded the timeout"
        );

        thread::sleep(Duration::from_millis(10));
    };

    let mut stdout = String::new();
    let mut stderr = String::new();

    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();

    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    assert!(
        status.success(),
        "alias case {case}: child failed ({status})\n{stdout}\n{stderr}"
    );

    // Prevent a false success if the child test was not actually selected.
    assert!(
        stdout.contains(PROBE_OK),
        "alias case {case}: probe did not confirm the expected error\n{stdout}"
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
                .unwrap();

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