//! Inspect bindings and signatures in the variable, function, and macro namespaces.
use super::*;


const ALL_FUNCTIONS_DOC: &str = "(all-functions): Return the names of every function visible \
     from the current environment, as a sorted list of strings -- primitives and Lisp \
     definitions alike, since nothing calling them can tell the difference.\n\n\
     A name bound in an inner scope appears once, not twice: this is the set of names that \
     would resolve, not the set of bindings that exist.\n\n\
     Macros are *not* included; see `all-macros'. Keeping them apart is what lets a caller \
     treat a macro as syntax and a function as a call.\n\n\
     Example:\n\
     (all-functions) => (\"1+\" \"abs\" \"append\" ...)";

name_list!(primitive_all_functions, function_names);

const ALL_MACROS_DOC: &str = "(all-macros): Return the names of every macro visible from the \
     current environment, as a sorted list of strings.\n\n\
     Special forms are not macros and are not listed: `if' and `let' are built into the \
     evaluator and have no binding to enumerate.\n\n\
     Example:\n\
     (all-macros) => (\"defcommand\")";

name_list!(primitive_all_macros, macro_names);

const ALL_VARIABLES_DOC: &str = "(all-variables): Return the names of every variable visible \
     from the current environment, as a sorted list of strings.\n\n\
     Called inside a `let', its bindings are in the list too -- they are variables, and they \
     would resolve. What comes back describes the environment it was asked in.\n\n\
     Example:\n\
     (all-variables) => (\"case-fold-search\" \"frame-width\" ...)";

name_list!(primitive_all_variables, variable_names);

const BOUNDP_DOC: &str = "(boundp SYMBOL): Return t if SYMBOL has a value in the current \
     environment, nil otherwise.\n\n\
     The other half of `functionp', which asks the same thing of the function namespace. A \
     symbol can be bound in one and not the other, or in neither and still be documented -- \
     see `variable-doc'.\n\n\
     Worth asking before `symbol-value', which refuses a name that is bound to nothing rather \
     than inventing nil for it.\n\n\
     Example:\n\
     (boundp 'case-fold-search) => t\n\
     (boundp 'no-such-variable) => nil";

fn primitive_boundp<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    _ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let LispExp::Symbol(name) = &args[0] else {
        return Err(EvalError::WrongArgumentType {
            expected: "Symbol".into(),
            got: args[0].clone(),
        });
    };
    Ok(LispExp::boolean(env.get_variable(name).is_some()))
}

const SYMBOL_VALUE_DOC: &str = "(symbol-value SYMBOL): Return what SYMBOL is bound to, without \
     evaluating anything else.\n\n\
     What a caller holding a *name* needs. Evaluating the symbol works when it was written down \
     in the source; it does not when the name arrived as data -- from `all-variables', from a \
     prompt, from a list being walked -- and this is the way to ask in that case.\n\n\
     Signals rather than answering nil when SYMBOL is bound to nothing, because nil is a value a \
     variable can legitimately have and the two must not read the same. Ask `boundp' first.\n\n\
     Example:\n\
     (symbol-value 'case-fold-search) => t\n\
     (mapcar 'symbol-value '(frame-width frame-height)) => (80 24)";

fn primitive_symbol_value<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    _ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let LispExp::Symbol(name) = &args[0] else {
        return Err(EvalError::WrongArgumentType {
            expected: "Symbol".into(),
            got: args[0].clone(),
        });
    };
    env.get_variable(name)
        .ok_or_else(|| EvalError::UnboundVariable(name.to_string()))
}

const FUNCTION_ARGLIST_DOC: &str = "(function-arglist SYMBOL): Return how the function bound to \
     SYMBOL is called, as a list of strings -- its required parameters, then \"&optional\" and \
     the optional ones, then \"&rest\" and the rest parameter -- exactly as they were written \
     in its parameter list.\n\n\
     Returns nil for a primitive: one is a Rust function and has no parameter list to read. \
     What a primitive's call looks like is in the first line of its documentation, which is \
     where `function-doc' finds it.\n\n\
     Returns nil, and logs, for a name that is no function at all.\n\n\
     Example:\n\
     (defun greet (who &optional loudly) nil)\n\
     (function-arglist 'greet) => (\"who\" \"&optional\" \"loudly\")";

fn primitive_function_arglist<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    if args.len() != 1 {
        let err = Err(EvalError::WrongNumberOfArguments {
            expected: 1,
            got: args.len(),
        });
        ctx.log_diagnostic(&format!("{err:?}"));
        return err;
    }
    let LispExp::Symbol(name) = &args[0] else {
        let err = Err(EvalError::WrongArgumentType {
            expected: "Symbol".into(),
            got: args[0].clone(),
        });
        ctx.log_diagnostic(&format!("{err:?}"));
        return err;
    };
    let Some(function) = env.get_function(name) else {
        ctx.log_diagnostic(&format!("{} names no function", name.as_str()));
        return Ok(nil!());
    };
    // Native function pointers carry no parameter names to report.
    let LispExp::Lambda(lambda) = function else {
        return Ok(nil!());
    };
    let mut parts: Vec<LispExp<T>> = lambda.params.iter().cloned().map(LispExp::string).collect();
    if !lambda.optionals.is_empty() {
        parts.push(LispExp::string("&optional".to_string()));
        parts.extend(lambda.optionals.iter().cloned().map(LispExp::string));
    }
    if let Some(rest) = &lambda.rest {
        parts.push(LispExp::string("&rest".to_string()));
        parts.push(LispExp::string(rest.clone()));
    }
    Ok(LispExp::proper_list(parts))
}

/// Register this module's primitives and Lisp docstrings.
pub(super) fn install<T: LispContext>(into: &Registry<T>) {
    into.function(
        "function-arglist",
        primitive_function_arglist,
        FUNCTION_ARGLIST_DOC,
    );
    into.function("boundp", primitive_boundp, BOUNDP_DOC);
    into.function("symbol-value", primitive_symbol_value, SYMBOL_VALUE_DOC);
    into.function("all-functions", primitive_all_functions, ALL_FUNCTIONS_DOC);
    into.function("all-macros", primitive_all_macros, ALL_MACROS_DOC);
    into.function("all-variables", primitive_all_variables, ALL_VARIABLES_DOC);
}
