//! Argument binding, syntax conversion, and condition handling helpers.

use super::{Env, EvalError, Lambda, LispContext, LispExp};
use std::sync::Arc;

/// Require exactly `count` arguments.
///
/// # Errors
///
/// Returns [`EvalError::WrongNumberOfArguments`] on a mismatch.
pub fn exact_arity<T: LispContext>(args: &[LispExp<T>], count: usize) -> Result<(), EvalError<T>> {
    if args.len() != count {
        return Err(EvalError::WrongNumberOfArguments {
            expected: count,
            got: args.len(),
        });
    }
    Ok(())
}

/// Require at least one argument.
///
/// # Errors
///
/// Returns [`EvalError::WrongNumberOfArguments`] with `expected: 1` when empty.
pub fn some_arguments<T: LispContext>(args: &[LispExp<T>]) -> Result<(), EvalError<T>> {
    if args.is_empty() {
        return Err(EvalError::WrongNumberOfArguments {
            expected: 1,
            got: 0,
        });
    }
    Ok(())
}

/// Convert list data into executable forms, preserving quoted payloads.
/// Visit lists and vectors iteratively; reject improper code lists.
pub(super) fn data_to_form<T: LispContext>(
    exp: &LispExp<T>,
) -> Result<LispExp<T>, EvalError<T>> {
    enum Task<'a, T: LispContext> {
        Visit(&'a LispExp<T>),
        FinishForm(usize),
        FinishVector(usize),
    }

    if !matches!(exp, LispExp::Cons(_) | LispExp::Vector(_)) {
        return Ok(exp.clone());
    }

    let mut pending = vec![Task::Visit(exp)];
    let mut converted = Vec::new();

    while let Some(task) = pending.pop() {
        match task {
            Task::Visit(value) => match value {
                LispExp::Cons(_) => {
                    let mut items = Vec::new();
                    let mut tail = value;

                    while let LispExp::Cons(cell) = tail {
                        items.push(&cell.car);
                        tail = &cell.cdr;
                    }

                    if !tail.is_nil() {
                        return Err(EvalError::UnvalidFunctionCall);
                    }

                    // A quoted payload must remain data, including dotted lists.
                    if items.len() == 2
                        && matches!(
                            items[0],
                            LispExp::Symbol(name) if name.as_str() == "quote"
                        )
                    {
                        converted.push(LispExp::form(
                            items.into_iter().cloned().collect(),
                        ));
                        continue;
                    }

                    pending.push(Task::FinishForm(items.len()));
                    pending.extend(items.into_iter().rev().map(Task::Visit));
                }

                LispExp::Vector(items) => {
                    pending.push(Task::FinishVector(items.len()));
                    pending.extend(items.iter().rev().map(Task::Visit));
                }

                other => converted.push(other.clone()),
            },

            Task::FinishForm(count) => {
                let items = converted.split_off(converted.len() - count);
                converted.push(LispExp::form(items));
            }

            Task::FinishVector(count) => {
                let items = converted.split_off(converted.len() - count);
                converted.push(LispExp::vec(items));
            }
        }
    }

    Ok(converted.pop().expect("conversion produces one root value"))
}

/// Bind required, optional, and rest arguments in the call frame.
///
/// Omitted optionals become `nil`; excess arguments form the rest list.
/// Returns an arity error when the parameter list cannot accept the arguments.
pub fn bind_lambda_args<T: LispContext>(
    lambda: &Lambda<T>,
    args: &[LispExp<T>],
    call_frame: &Arc<Env<T>>,
) -> Result<(), EvalError<T>> {
    let min = lambda.params.len();
    let max = min + lambda.optionals.len();
    if args.len() < min || (lambda.rest.is_none() && args.len() > max) {
        return Err(EvalError::WrongNumberOfArguments {
            expected: if args.len() < min { min } else { max },
            got: args.len(),
        });
    }

    let mut idx = 0;
    for name in &lambda.params {
        call_frame.set_variable(name.clone(), args[idx].clone());
        idx += 1;
    }
    for name in &lambda.optionals {
        let value = args.get(idx).cloned().unwrap_or_else(LispExp::nil);
        call_frame.set_variable(name.clone(), value);
        idx += 1;
    }
    if let Some(rest_name) = &lambda.rest {
        // Missing optional arguments can advance `idx` beyond `args.len()`;
        // clamp it before slicing the rest arguments.
        let rest_start = idx.min(args.len());
        call_frame.set_variable(
            rest_name.clone(),
            // Even for macros, the rest-argument container is data whose elements may be syntax.
            LispExp::proper_list(args[rest_start..].to_vec()),
        );
    }
    Ok(())
}

/// Parse required, optional, and rest parameter names, validating marker order.
/// Reject self-evaluating symbols, which cannot be read as lexical bindings.
pub(super) fn parse_lambda_params<T: LispContext>(
    params_list: &[LispExp<T>],
) -> Result<(Vec<String>, Vec<String>, Option<String>), EvalError<T>> {
    #[derive(PartialEq)]
    enum Mode {
        Required,
        Optional,
        Rest,
        RestDone,
    }

    let mut required = Vec::new();
    let mut optionals = Vec::new();
    let mut rest = None;
    let mut mode = Mode::Required;

    for param in params_list {
        let LispExp::Symbol(name) = param else {
            return Err(EvalError::DefunParamIsNotASymbol);
        };
        if matches!(name.as_str(), "t" | "nil") || name.starts_with(':') {
            return Err(EvalError::DefunInvalidParameterName(name.to_string()));
        }
        match name.as_str() {
            "&optional" => {
                if mode != Mode::Required {
                    return Err(EvalError::DefunMisplacedParamMarker);
                }
                mode = Mode::Optional;
            }
            "&rest" => {
                if mode == Mode::Rest || mode == Mode::RestDone {
                    return Err(EvalError::DefunMisplacedParamMarker);
                }
                mode = Mode::Rest;
            }
            other => match mode {
                Mode::Required => required.push(other.to_string()),
                Mode::Optional => optionals.push(other.to_string()),
                Mode::Rest => {
                    rest = Some(other.to_string());
                    mode = Mode::RestDone;
                }
                Mode::RestDone => return Err(EvalError::DefunRestMustHaveExactlyOneParam),
            },
        }
    }

    if mode == Mode::Rest {
        return Err(EvalError::DefunRestMustHaveExactlyOneParam);
    }

    Ok((required, optionals, rest))
}

/// Map an evaluation failure to its Lisp condition symbol.
/// Malformed special forms use the generic `error` condition.
pub(super) fn error_symbol<T: LispContext>(err: &EvalError<T>) -> LispExp<T> {
    let name = match err {
        EvalError::Signal { symbol, .. } => return symbol.clone(),
        EvalError::UnboundVariable(_) => "unbound-variable",
        EvalError::UndefinedFunction(_) => "undefined-function",
        EvalError::UnvalidFunctionCall | EvalError::UncorrectFunctionDefinition => {
            "invalid-function"
        }
        EvalError::WrongNumberOfArguments { .. } => "wrong-number-of-arguments",
        EvalError::WrongArgumentType { .. } => "wrong-type-argument",
        EvalError::DefunInvalidParameterName(_) => "invalid-parameter-name",
        EvalError::OutOfFuel => "out-of-fuel",
        EvalError::EvalDepthExceeded => "eval-depth-exceeded",
        EvalError::RuntimeMessage(_) => "runtime-error",
        _ => "error",
    };
    LispExp::symbol(name.into())
}

/// Return condition data containing the offending values for Lisp handlers.
pub(super) fn error_data<T: LispContext>(err: &EvalError<T>) -> LispExp<T> {
    match err {
        EvalError::Signal { data, .. } => data.clone(),
        EvalError::WrongArgumentType { expected, got } => {
            LispExp::proper_list(vec![LispExp::string(expected.clone()), got.clone()])
        }
        EvalError::UnboundVariable(name)
        | EvalError::UndefinedFunction(name)
        | EvalError::DefunInvalidParameterName(name) => {
            LispExp::proper_list(vec![LispExp::symbol(name.clone())])
        }
        EvalError::WrongNumberOfArguments { expected, got } => LispExp::proper_list(vec![
            LispExp::number(*expected as f64),
            LispExp::number(*got as f64),
        ]),
        EvalError::RuntimeMessage(msg) => LispExp::proper_list(vec![LispExp::string(msg.clone())]),
        other => LispExp::proper_list(vec![LispExp::string(format!("{:?}", other))]),
    }
}

/// Match a condition symbol or syntax list without recursive traversal.
/// The symbols t and error match all conditions.
pub(super) fn condition_matches<T: LispContext>(
    condition: &LispExp<T>,
    symbol: &LispExp<T>,
) -> bool {
    let mut current = condition;
    let mut pending = Vec::new();

    loop {
        match current {
            LispExp::Symbol(name) => {
                if matches!(name.as_str(), "t" | "error") || current == symbol {
                    return true;
                }
            }
            // Handler conditions are syntax forms, not runtime cons lists.
            LispExp::Form(items) => pending.extend(items.iter().rev()),
            _ => {}
        }

        match pending.pop() {
            Some(next) => current = next,
            None => return false,
        }
    }
}

/// Only these variants participate in this structural drain. Runtime-owned
/// objects (atoms, closures, environments and continuations) keep their own
/// lifetime behavior.
pub(crate) fn is_container<T: LispContext>(value: &LispExp<T>) -> bool {
    matches!(
        value,
        LispExp::Cons(_) | LispExp::Form(_) | LispExp::Vector(_) | LispExp::Map(_)
    )
}

/// Detach a structural child without interning or changing nil's refcount.
/// Other fields stay in place and are released by normal field destruction.
pub(crate) fn take_container<T: LispContext>(value: &mut LispExp<T>) -> Option<LispExp<T>> {
    if is_container(value) {
        Some(std::mem::replace(value, LispExp::Number(0.0)))
    } else {
        None
    }
}
