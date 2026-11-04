#![cfg(test)]

use crate::{
    Env, EvalError, Frame, LispContext, LispExp, Parser, eval, setup_base_env,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, PartialEq)]
struct Host;

impl LispContext for Host {}

struct DropProbe {
    check: Box<dyn Fn() -> bool + Send + Sync>,
    observed: Arc<AtomicBool>,
}

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.observed.store((self.check)(), Ordering::SeqCst);
    }
}

fn watched_value(
    check: impl Fn() -> bool + Send + Sync + 'static,
) -> (LispExp<Host>, Arc<AtomicBool>) {
    let observed = Arc::new(AtomicBool::new(false));
    let probe = DropProbe {
        check: Box::new(check),
        observed: observed.clone(),
    };

    let value = LispExp::native(
        move |_, _, _| {
            std::hint::black_box(&probe);
            Ok(LispExp::nil())
        },
        None,
    );

    (value, observed)
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
fn replacing_variable_releases_lock_before_drop() {
    let env = Env::new_root();
    let weak = Arc::downgrade(&env);
    let (old, observed) = watched_value(move || {
        let Some(env) = weak.upgrade() else { return false };
        let unlocked = env.variables.try_read().is_ok();
        unlocked && env.get_variable("watched") == Some(LispExp::number(42.0))
    });

    env.set_variable("watched".into(), old);
    env.set_variable("watched".into(), LispExp::number(42.0));

    assert!(observed.load(Ordering::SeqCst));
}

#[test]
fn replacing_function_releases_lock_before_drop() {
    let env = Env::new_root();
    let weak = Arc::downgrade(&env);
    let (old, observed) = watched_value(move || {
        let Some(env) = weak.upgrade() else { return false };
        let unlocked = env.functions.try_read().is_ok();
        unlocked && env.get_function("watched") == Some(LispExp::number(42.0))
    });

    env.set_function("watched".into(), old);
    env.set_function("watched".into(), LispExp::number(42.0));

    assert!(observed.load(Ordering::SeqCst));
}

#[test]
fn replacing_macro_releases_lock_before_drop() {
    let env = Env::new_root();
    let weak = Arc::downgrade(&env);
    let (old, observed) = watched_value(move || {
        let Some(env) = weak.upgrade() else { return false };
        let unlocked = env.macros.try_read().is_ok();
        unlocked && env.get_macro("watched") == Some(LispExp::number(42.0))
    });

    env.set_macro("watched".into(), old);
    env.set_macro("watched".into(), LispExp::number(42.0));

    assert!(observed.load(Ordering::SeqCst));
}

#[test]
fn updating_parent_releases_all_variable_locks_before_drop() {
    let root = Env::new_root();
    let child = Env::new_child(&root);
    let weak_root = Arc::downgrade(&root);
    let weak_child = Arc::downgrade(&child);

    let (old, observed) = watched_value(move || {
        let (Some(root), Some(child)) =
            (weak_root.upgrade(), weak_child.upgrade())
        else {
            return false;
        };
        let unlocked = root.variables.try_read().is_ok()
            && child.variables.try_read().is_ok();
        unlocked && child.get_variable("watched") == Some(LispExp::number(42.0))
    });

    root.set_variable("watched".into(), old);
    assert!(child.update_variable("watched", LispExp::number(42.0)));

    assert!(observed.load(Ordering::SeqCst));
    assert_eq!(root.get_variable("watched"), Some(LispExp::number(42.0)));
    assert!(!child.variables.read().unwrap().contains_key("watched"));
}

#[test]
fn failed_update_drops_argument_without_holding_variable_locks() {
    let root = Env::new_root();
    let child = Env::new_child(&root);
    let weak_root = Arc::downgrade(&root);
    let weak_child = Arc::downgrade(&child);

    let (value, observed) = watched_value(move || {
        let (Some(root), Some(child)) =
            (weak_root.upgrade(), weak_child.upgrade())
        else {
            return false;
        };
        let unlocked = root.variables.try_read().is_ok()
            && child.variables.try_read().is_ok();
        unlocked && child.get_variable("missing").is_none()
    });

    assert!(!child.update_variable("missing", value));
    assert!(observed.load(Ordering::SeqCst));
}

#[test]
fn replacing_property_releases_root_lock_before_drop() {
    let root = Env::new_root();
    let child = Env::new_child(&root);
    let weak = Arc::downgrade(&root);

    let (old, observed) = watched_value(move || {
        let Some(root) = weak.upgrade() else { return false };
        let unlocked = root.properties.as_ref().unwrap().try_read().is_ok();
        unlocked
            && root.get_property("subject", "watched")
                == Some(LispExp::number(42.0))
    });

    root.put_property("subject", "watched", old);
    child.put_property("subject", "watched", LispExp::number(42.0));

    assert!(observed.load(Ordering::SeqCst));
}

#[test]
fn resetting_atom_releases_lock_before_drop() {
    let env = Env::new_root();
    setup_base_env(env.clone());
    let atom = eval_source("(make-atom nil)", env.clone()).unwrap();
    let storage = match &atom {
        LispExp::Atom(atom) => atom.0.clone(),
        _ => panic!("expected atom"),
    };

    let weak_storage = Arc::downgrade(&storage);
    let weak_env = Arc::downgrade(&env);
    let (old, observed) = watched_value(move || {
        let (Some(storage), Some(env)) =
            (weak_storage.upgrade(), weak_env.upgrade())
        else {
            return false;
        };
        let unlocked = storage.try_read().is_ok();
        unlocked && eval_source("(deref a)", env) == Ok(LispExp::number(42.0))
    });

    *storage.write().unwrap() = old;
    env.set_variable("a".into(), atom);

    assert_eq!(
        eval_source("(reset a 42)", env),
        Ok(LispExp::number(42.0)),
    );
    assert!(observed.load(Ordering::SeqCst));
}

#[test]
fn publishing_fiber_frames_releases_lock_before_retiring_old_frames() {
    let env = Env::new_root();
    setup_base_env(env.clone());

    let fiber = eval_source(
        "(fiber (install-retired) (yield 1))",
        env.clone(),
    ).unwrap();
    let weak_fiber = match &fiber {
        LispExp::Fiber(fiber) => Arc::downgrade(&fiber.0),
        _ => panic!("expected fiber"),
    };
    let weak_env = Arc::downgrade(&env);
    env.set_variable("f".into(), fiber);

    let (old, observed) = watched_value(move || {
        let (Some(fiber), Some(env)) =
            (weak_fiber.upgrade(), weak_env.upgrade())
        else {
            return false;
        };
        let unlocked = fiber.try_read().is_ok();
        unlocked && eval_source("(fiber-done-p f)", env) == Ok(LispExp::nil())
    });

    let staged = Mutex::new(Some(old));
    env.set_function(
        "install-retired".into(),
        LispExp::native(
            move |_, env, _| {
                let value = staged.lock().unwrap().take().unwrap();
                let fiber = match env.get_variable("f") {
                    Some(LispExp::Fiber(fiber)) => fiber,
                    _ => panic!("expected fiber binding"),
                };

                // Exercise replacement of state populated by an embedding host.
                fiber.0.write().unwrap().pending.push(Frame::Body {
                    forms: vec![value],
                    from: 0,
                    env: Env::new_root(),
                });
                Ok(LispExp::nil())
            },
            None,
        ),
    );

    let first = eval_source("(resume f)", env.clone());
    let second = eval_source("(resume f)", env);

    assert_eq!(first, Ok(LispExp::number(1.0)));
    assert_eq!(second, Ok(LispExp::nil()));
    assert!(observed.load(Ordering::SeqCst));
}