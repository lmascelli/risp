//! Host-independent Lisp primitives, grouped by domain.
//!
//! Shared macros and argument helpers live here; each submodule registers its
//! own implementations and Lisp docstrings through [`setup_base_env`].

use crate::{
    Env, EvalError, LispContext, LispExp, Parser, VARIABLE_DOCUMENTATION, bind_lambda_args, eval,
    exact_arity, resume_frames, some_arguments,
};
use std::sync::{Arc, RwLock};

macro_rules! nil {
    () => {
        LispExp::nil()
    };
}
use crate::{LispPrimitive, SharedAtom};

// Generate function items because primitives are registered as bare function pointers.

/// A primitive answering t or nil for one argument's shape.
macro_rules! predicate {
    ($name:ident, $pattern:pat) => {
        fn $name<T: LispContext>(
            args: &[LispExp<T>],
            _env: Arc<Env<T>>,
            _ctx: &T,
        ) -> Result<LispExp<T>, EvalError<T>> {
            exact_arity(args, 1)?;
            Ok(LispExp::boolean(matches!(&args[0], $pattern)))
        }
    };
    ($name:ident, not $pattern:pat) => {
        fn $name<T: LispContext>(
            args: &[LispExp<T>],
            _env: Arc<Env<T>>,
            _ctx: &T,
        ) -> Result<LispExp<T>, EvalError<T>> {
            exact_arity(args, 1)?;
            Ok(LispExp::boolean(!matches!(&args[0], $pattern)))
        }
    };
}

/// A primitive taking one number and giving one back.
macro_rules! number_op {
    ($name:ident, $method:ident) => {
        fn $name<T: LispContext>(
            args: &[LispExp<T>],
            _env: Arc<Env<T>>,
            _ctx: &T,
        ) -> Result<LispExp<T>, EvalError<T>> {
            exact_arity(args, 1)?;
            Ok(LispExp::number(expect_number(&args[0])?.$method()))
        }
    };
}

/// A primitive taking one string and giving one back.
macro_rules! string_op {
    ($name:ident, $method:ident) => {
        fn $name<T: LispContext>(
            args: &[LispExp<T>],
            _env: Arc<Env<T>>,
            _ctx: &T,
        ) -> Result<LispExp<T>, EvalError<T>> {
            exact_arity(args, 1)?;
            Ok(LispExp::string(expect_string(&args[0])?.$method()))
        }
    };
}

/// A primitive folding one or more numbers down to one.
macro_rules! fold_numbers {
    ($name:ident, $method:ident) => {
        fn $name<T: LispContext>(
            args: &[LispExp<T>],
            _env: Arc<Env<T>>,
            _ctx: &T,
        ) -> Result<LispExp<T>, EvalError<T>> {
            some_arguments(args)?;
            let mut folded = expect_number(&args[0])?;
            for arg in &args[1..] {
                folded = folded.$method(expect_number(arg)?);
            }
            Ok(LispExp::number(folded))
        }
    };
}

/// A primitive listing every name in one of the environment's namespaces.
macro_rules! name_list {
    ($name:ident, $names:ident) => {
        fn $name<T: LispContext>(
            _args: &[LispExp<T>],
            env: Arc<Env<T>>,
            _ctx: &T,
        ) -> Result<LispExp<T>, EvalError<T>> {
            Ok(LispExp::proper_list(
                env.$names().into_iter().map(LispExp::string).collect(),
            ))
        }
    };
}

/// A primitive that does nothing but raise its own kind of non-local exit.
macro_rules! raise {
    ($name:ident, $variant:ident { $first:ident, $second:ident }) => {
        fn $name<T: LispContext>(
            args: &[LispExp<T>],
            _env: Arc<Env<T>>,
            _ctx: &T,
        ) -> Result<LispExp<T>, EvalError<T>> {
            exact_arity(args, 2)?;
            Err(EvalError::$variant {
                $first: args[0].clone(),
                $second: args[1].clone(),
            })
        }
    };
}

fn expect_number<T: LispContext>(exp: &LispExp<T>) -> Result<f64, EvalError<T>> {
    if let LispExp::Number(n) = exp {
        Ok(*n)
    } else {
        Err(EvalError::WrongArgumentType {
            expected: "Number".into(),
            got: exp.clone(),
        })
    }
}

/// Charge the host for `units` of work, saturating at `u32::MAX`.
fn charge<T: LispContext>(ctx: &T, units: usize) -> Result<(), EvalError<T>> {
    ctx.consume_fuel(u32::try_from(units).unwrap_or(u32::MAX))
}

/// Collect a list argument into a vector, charging one fuel unit per element.
///
/// Charges the proper prefix after collection; a dotted tail is ignored.
fn expect_list<T: LispContext>(exp: &LispExp<T>, ctx: &T) -> Result<Vec<LispExp<T>>, EvalError<T>> {
    match exp {
        LispExp::Cons(_) => {
            let items: Vec<LispExp<T>> = exp.iter().collect();
            charge(ctx, items.len())?;
            Ok(items)
        }
        other if other.is_nil() => Ok(vec![]),
        other => Err(EvalError::WrongArgumentType {
            expected: "List".into(),
            got: other.clone(),
        }),
    }
}

/// Accept cons values, including dotted lists, and nil as list-valued bindings.
fn is_list_value<T: LispContext>(exp: &LispExp<T>) -> bool {
    matches!(exp, LispExp::Cons(_)) || exp.is_nil()
}

fn expect_string<T: LispContext>(exp: &LispExp<T>) -> Result<String, EvalError<T>> {
    if let LispExp::String(s) = exp {
        Ok((**s).clone())
    } else {
        Err(EvalError::WrongArgumentType {
            expected: "String".into(),
            got: exp.clone(),
        })
    }
}

fn format_number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Format a value for display: strings without quotes, symbols by name, and
/// collections in Lisp notation. Numbers use the display formatter.
///
/// This is presentation text, not a serialization format.
pub fn lisp_display<T: LispContext>(exp: &LispExp<T>) -> String {
    match exp {
        LispExp::String(s) => (**s).clone(),
        LispExp::Symbol(s) => (**s).clone(),
        LispExp::Number(n) => format_number(*n),
        other if other.is_nil() => "nil".into(),
        other => format!("{:?}", other),
    }
}

/// Invoke a lambda, primitive, or symbol in the function namespace.
///
/// `call_args` must already be evaluated. A lambda runs in its captured scope;
/// native functions receive `env`. Symbol aliases resolve in `env`'s function
/// namespace. This helper does not push a backtrace frame for the target or
/// open a fuel scope. Lisp yields from these callbacks are not supported.
///
/// # Errors
///
/// Returns [`EvalError::UndefinedFunction`] for an unbound function symbol,
/// [`EvalError::UncorrectFunctionDefinition`] for a non-callable value, or
/// propagates argument and evaluation failures from the callee.
pub fn call_callable<T: LispContext>(
    func: &LispExp<T>,
    call_args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    match func {
        LispExp::Lambda(lambda) => {
            let call_frame = Env::new_child(&lambda.env);
            bind_lambda_args(lambda, call_args, &call_frame)?;
            if lambda.body.is_empty() {
                return Ok(LispExp::nil());
            }
            for exp in &lambda.body[0..lambda.body.len() - 1] {
                eval(exp, call_frame.clone(), ctx)?;
            }
            eval(
                lambda
                    .body
                    .last()
                    .expect("Failed to get the last expression in the function call"),
                call_frame,
                ctx,
            )
        }
        LispExp::Primitive { pointer: f, doc: _ } => f(call_args, env, ctx),
        LispExp::Native { callback, .. } => callback(call_args, env, ctx),
        LispExp::Symbol(name) => {
            if let Some(resolved) = env.get_function(name) {
                call_callable(&resolved, call_args, env, ctx)
            } else {
                Err(EvalError::UndefinedFunction(name.to_string()))
            }
        }
        _ => Err(EvalError::UncorrectFunctionDefinition),
    }
}

fn find_assoc<T: LispContext>(args: &[LispExp<T>], ctx: &T) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 2)?;
    let list = expect_list(&args[1], ctx)?;
    for entry in list {
        let key = match &entry {
            LispExp::Cons(cell) => Some(cell.car.clone()),
            _ => None,
        };
        if key == Some(args[0].clone()) {
            return Ok(entry);
        }
    }
    Ok(LispExp::nil())
}

fn find_member<T: LispContext>(args: &[LispExp<T>], ctx: &T) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 2)?;
    let list = expect_list(&args[1], ctx)?;
    if let Some(pos) = list.iter().position(|e| *e == args[0]) {
        Ok(LispExp::proper_list(list[pos..].to_vec()))
    } else {
        Ok(LispExp::nil())
    }
}

fn compare_chain<T: LispContext>(
    args: &[LispExp<T>],
    op: fn(f64, f64) -> bool,
) -> Result<LispExp<T>, EvalError<T>> {
    some_arguments(args)?;
    let mut numbers = Vec::with_capacity(args.len());
    for arg in args {
        numbers.push(expect_number(arg)?);
    }
    Ok(LispExp::boolean(numbers.windows(2).all(|w| op(w[0], w[1]))))
}

// Submodules must follow the macros they use because `macro_rules!` has textual scope.
mod atoms;
mod comparisons;
mod fibers;
mod functions;
mod inspect;
mod lists;
mod math;
mod predicates;
mod strings;
mod symbols;

/// Register native functions and their Lisp docstrings in an environment.
pub(crate) struct Registry<'a, T: LispContext> {
    env: &'a Arc<Env<T>>,
}

impl<T: LispContext> Registry<'_, T> {
    /// Register a primitive with documentation starting `(name ARGS): ...`.
    /// Native parameter names are unavailable to Lisp introspection.
    pub(crate) fn function(&self, name: &str, pointer: LispPrimitive<T>, doc: &'static str) {
        self.env
            .set_function(name.into(), LispExp::primitive(pointer, Some(doc.into())));
    }
}

/// Install the standard Lisp primitives into `env`.
///
/// Existing function bindings with the same names are replaced.
pub fn setup_base_env<T: LispContext>(env: Arc<Env<T>>) {
    let into = Registry { env: &env };
    atoms::install(&into);
    comparisons::install(&into);
    fibers::install(&into);
    functions::install(&into);
    inspect::install(&into);
    lists::install(&into);
    math::install(&into);
    predicates::install(&into);
    strings::install(&into);
    symbols::install(&into);
}
