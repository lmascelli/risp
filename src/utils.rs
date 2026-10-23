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
/// Reject improper code lists; recurse into vector contents.
pub(super) fn data_to_form<T: LispContext>(exp: &LispExp<T>) -> Result<LispExp<T>, EvalError<T>> {
    match exp {
        LispExp::Cons(_) => {
            let (items, tail) = exp.split_list();
            if !tail.is_nil() {
                return Err(EvalError::UnvalidFunctionCall);
            }
            // Keep the argument of `quote` as data, matching the reader's representation.
            if items.len() == 2 {
                if matches!(&items[0], LispExp::Symbol(s) if s.as_str() == "quote") {
                    return Ok(LispExp::form(vec![items[0].clone(), items[1].clone()]));
                }
            }
            let mut form = Vec::with_capacity(items.len());
            for item in &items {
                form.push(data_to_form(item)?);
            }
            Ok(LispExp::form(form))
        }
        LispExp::Vector(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items.iter() {
                out.push(data_to_form(item)?);
            }
            Ok(LispExp::vec(out))
        }
        other => Ok(other.clone()),
    }
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
        EvalError::UnboundVariable(name) | EvalError::UndefinedFunction(name) => {
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

/// Match a condition symbol or syntax list; `t` and `error` match all conditions.
pub(super) fn condition_matches<T: LispContext>(
    condition: &LispExp<T>,
    symbol: &LispExp<T>,
) -> bool {
    match condition {
        LispExp::Symbol(name) => {
            let name = name.as_str();
            name == "t" || name == "error" || condition == symbol
        }
        // Handler conditions are unevaluated syntax, so a list of them is a
        // `Form`, not a `Cons`.
        LispExp::Form(items) => items.iter().any(|c| condition_matches(c, symbol)),
        _ => false,
    }
}
