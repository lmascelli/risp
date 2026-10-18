//! Expression evaluation, tail calls, and resumable statement continuations.

use super::{
    Env, EvalError, FiberState, Frame, Lambda, LispContext, LispExp, VARIABLE_DOCUMENTATION,
    bind_lambda_args, condition_matches, data_to_form, error_data, error_symbol,
    parse_lambda_params,
};
use std::{cell::Cell, collections::HashMap, sync::Arc};

enum EvalStep<T: LispContext> {
    Done(LispExp<T>),
    TailCall(LispExp<T>, Arc<Env<T>>),
}

thread_local! {
    /// One-use permission to suspend the next evaluation on this thread.
    /// `eval_step` consumes it before evaluating subexpressions, preventing a
    /// statement's permission from leaking into an argument. Tail calls preserve it.
    static YIELD_PERMITTED: Cell<bool> = const { Cell::new(false) };
}

/// Allow the next evaluation on this thread to suspend.
///
/// Only callers that can record a continuation may grant permission.
pub fn grant_yield_permission() {
    YIELD_PERMITTED.set(true);
}

/// Take the permission granted for this evaluation, leaving none for anything
/// nested inside it.
fn take_yield_permission() -> bool {
    YIELD_PERMITTED.replace(false)
}

/// Evaluate an expression in `env` using host services from `ctx`.
///
/// Nonconstant symbols resolve as variables; named calls resolve as special
/// forms, macros, or functions. Data cons lists are converted to code before
/// evaluation. Tail calls use a trampoline.
///
/// # Errors
///
/// Returns [`EvalError`] for invalid forms, failed lookups, rejected fuel charges,
/// callee failures, or non-local control transfers that escape their enclosing form.
///
/// Fuel is charged through [`LispContext::consume_fuel`]; this function does
/// not open or refill a metered scope. Failed calls may leave host backtrace
/// frames for the caller to report and clear.
pub fn eval<T: LispContext>(
    exp: &LispExp<T>,
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    let mut current_exp = exp.clone();
    let mut current_env = env;

    loop {
        match eval_step(&current_exp, current_env.clone(), ctx) {
            Ok(EvalStep::Done(result)) => return Ok(result),
            Ok(EvalStep::TailCall(next_exp, next_env)) => {
                current_exp = next_exp;
                current_env = next_env;
            }
            Err(error) => {
                note_error_origin(&current_exp, &error, ctx);
                return Err(error);
            }
        }
    }
}

/// Report the nearest located form for a failure, innermost first.
///
/// Unlocated forms are skipped. Throws and yields are control transfers,
/// so they do not establish an error origin.
#[cold]
#[inline(never)]
fn note_error_origin<T: LispContext>(exp: &LispExp<T>, error: &EvalError<T>, ctx: &T) {
    match error {
        EvalError::Throw { .. } | EvalError::Yielded { .. } => {
            return;
        }
        _ => (),
    }
    if let LispExp::Form(form) = exp {
        if let Some(at) = ctx.source_map().and_then(|map| map.locate(form)) {
            ctx.note_error_origin(&at);
        }
    }
}

/// Record the source location of a failed call whose frame remains pushed.
/// Suspended calls have already popped their frame and are skipped.
#[cold]
#[inline(never)]
fn note_call_site<T: LispContext>(form: &Arc<Vec<LispExp<T>>>, error: &EvalError<T>, ctx: &T) {
    if let EvalError::Yielded { .. } = error {
        return;
    }

    if let Some(at) = ctx.source_map().and_then(|map| map.locate(form)) {
        ctx.note_call_site(&at);
    }
}

fn eval_step<T: LispContext>(
    exp: &LispExp<T>,
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<EvalStep<T>, EvalError<T>> {
    let permitted = take_yield_permission();
    let step = eval_step_permitted(exp, env, ctx, permitted);
    // A tail call continues the same statement, so its yield permission carries over.
    if permitted && matches!(step, Ok(EvalStep::TailCall(..))) {
        grant_yield_permission();
    }
    step
}

/// Evaluate statements from `from`, recording a continuation if one yields.
///
/// `frame_at` records the next statement in a body or loop. Returns the last
/// value, or `carried` when there are no statements left.
fn run_statements<T: LispContext, F>(
    forms: &[LispExp<T>],
    from: usize,
    env: &Arc<Env<T>>,
    ctx: &T,
    permitted: bool,
    carried: LispExp<T>,
    frame_at: F,
) -> Result<LispExp<T>, EvalError<T>>
where
    F: Fn(usize) -> Frame<T>,
{
    // An empty outer block must preserve the value produced by resumed inner frames.
    let mut last = carried;
    for (index, form) in forms.iter().enumerate().skip(from) {
        if permitted {
            grant_yield_permission();
        }
        match eval(form, env.clone(), ctx) {
            Ok(value) => last = value,
            Err(EvalError::Yielded { value, mut frames }) => {
                // Append outer continuations after inner ones to preserve resumption order.
                frames.push(frame_at(index + 1));
                return Err(EvalError::Yielded { value, frames });
            }
            Err(other) => return Err(other),
        }
    }
    Ok(last)
}

/// Evaluate all but the last body form as statements; return the last as a tail call.
fn eval_body_step<T: LispContext>(
    body: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
    permitted: bool,
) -> Result<EvalStep<T>, EvalError<T>> {
    let Some((last, leading)) = body.split_last() else {
        return Ok(EvalStep::Done(LispExp::nil()));
    };
    let forms = body;
    run_statements(
        &forms[..leading.len()],
        0,
        &env,
        ctx,
        permitted,
        LispExp::nil(),
        |resume_from| Frame::Body {
            forms: forms.to_vec(),
            from: resume_from,
            env: env.clone(),
        },
    )?;
    Ok(EvalStep::TailCall(last.clone(), env))
}

/// Resume recorded blocks, innermost first, until completion or another yield.
///
/// If a block yields again, append the unvisited outer frames to the new
/// continuation. An empty stack returns nil. This consumes the supplied stack
/// without updating any fiber object or refilling fuel.
///
/// # Errors
///
/// Returns [`EvalError::Yielded`] with the new value and frames on suspension,
/// or propagates an evaluation failure. Prefer invoking the Lisp `resume`
/// function through [`crate::call_callable`] to update a fiber's stored state.
pub fn resume_frames<T: LispContext>(
    frames: Vec<Frame<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    let mut last = LispExp::nil();
    let mut outer = frames.into_iter();
    while let Some(frame) = outer.next() {
        let finished = match frame {
            Frame::Body { forms, from, env } => {
                run_statements(&forms, from, &env, ctx, true, last.clone(), |resume_from| {
                    Frame::Body {
                        forms: forms.clone(),
                        from: resume_from,
                        env: env.clone(),
                    }
                })
            }
            Frame::While {
                condition,
                forms,
                from,
                env,
            } => run_while(&condition, &forms, from, &env, ctx, last.clone()),
        };
        match finished {
            Ok(value) => last = value,
            Err(EvalError::Yielded { value, mut frames }) => {
                frames.extend(outer);
                return Err(EvalError::Yielded { value, frames });
            }
            Err(other) => return Err(other),
        }
    }
    Ok(last)
}

/// Finish the interrupted `while` iteration from `from`, then retest the condition.
fn run_while<T: LispContext>(
    condition: &LispExp<T>,
    forms: &[LispExp<T>],
    from: usize,
    env: &Arc<Env<T>>,
    ctx: &T,
    carried: LispExp<T>,
) -> Result<LispExp<T>, EvalError<T>> {
    let frame_at = |resume_from: usize| Frame::While {
        condition: condition.clone(),
        forms: forms.to_vec(),
        from: resume_from,
        env: env.clone(),
    };
    let mut last = run_statements(forms, from, env, ctx, true, carried, frame_at)?;
    loop {
        // A condition cannot yield because no continuation records a partial test.
        if eval(condition, env.clone(), ctx)?.is_nil() {
            return Ok(last);
        }
        last = run_statements(forms, 0, env, ctx, true, LispExp::nil(), frame_at)?;
    }
}

fn eval_step_permitted<T: LispContext>(
    exp: &LispExp<T>,
    env: Arc<Env<T>>,
    ctx: &T,
    permitted: bool,
) -> Result<EvalStep<T>, EvalError<T>> {
    ctx.consume_fuel(1)?;
    match exp {
        LispExp::String(_)
        | LispExp::Number(_)
        | LispExp::Atom(_)
        | LispExp::Fiber(_)
        | LispExp::Lambda(_) => Ok(EvalStep::Done(exp.clone())),

        LispExp::Primitive { pointer: _, doc: _ } => Ok(EvalStep::Done(exp.clone())),

        LispExp::Symbol(symbol) => {
            if symbol.as_str() == "nil" || symbol.as_str() == "t" || symbol.starts_with(':') {
                return Ok(EvalStep::Done(exp.clone()));
            }
            if let Some(var) = env.get_variable(symbol) {
                Ok(EvalStep::Done(var))
            } else {
                Err(EvalError::UnboundVariable(symbol.to_string()))
            }
        }

        LispExp::Cons(_) => Ok(EvalStep::TailCall(data_to_form(exp)?, env)),

        LispExp::Form(list) => {
            if list.is_empty() {
                Ok(EvalStep::Done(LispExp::nil()))
            } else {
                let head = &list[0];
                match head {
                    LispExp::Symbol(symbol) => eval_special_form_or_call_step(
                        symbol,
                        &list[1..],
                        env.clone(),
                        ctx,
                        permitted,
                    ),

                    LispExp::Form(_) => {
                        let mut new_ast = vec![eval(head, env.clone(), ctx)?];
                        for arg in &list[1..] {
                            new_ast.push(arg.clone());
                        }
                        return Ok(EvalStep::TailCall(LispExp::form(new_ast), env.clone()));
                    }

                    LispExp::Lambda(lambda) => {
                        let mut evaled_args = Vec::new();
                        for arg in &list[1..] {
                            evaled_args.push(eval(arg, env.clone(), ctx)?);
                        }

                        let call_frame = Env::new_child(&lambda.env);
                        ctx.push_call_frame("<lambda>");
                        bind_lambda_args(lambda, &evaled_args, &call_frame)?;

                        let step = eval_body_step(&lambda.body, call_frame, ctx, permitted);
                        pop_unless_failed(ctx, &step);
                        return step;
                    }
                    _ => {
                        return Err(EvalError::UnvalidFunctionCall);
                    }
                }
            }
        }

        LispExp::Vector(vec) => {
            let mut new_vec = Vec::with_capacity(vec.len());
            for v in vec.iter() {
                new_vec.push(eval(v, env.clone(), ctx)?);
            }
            Ok(EvalStep::Done(LispExp::vec(new_vec)))
        }

        LispExp::Map(map) => {
            let mut new_map = HashMap::new();
            for (k, v) in map.iter() {
                new_map.insert(k.clone(), eval(v, env.clone(), ctx)?);
            }
            Ok(EvalStep::Done(LispExp::map(new_map)))
        }
    }
}

/// Pop successful, tail-called, or suspended calls; retain failed calls for backtraces.
fn pop_unless_failed<T: LispContext>(ctx: &T, step: &Result<EvalStep<T>, EvalError<T>>) {
    if !matches!(step, Err(error) if !matches!(error, EvalError::Yielded { .. })) {
        ctx.pop_call_frame();
    }
}

fn eval_special_form_or_call_step<T: LispContext>(
    symbol: &str,
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
    permitted: bool,
) -> Result<EvalStep<T>, EvalError<T>> {
    match symbol {
        "quote" => {
            if args.len() != 1 {
                Err(EvalError::QuoteNotOneArgument)
            } else {
                Ok(EvalStep::Done(args[0].clone()))
            }
        }

        "if" => {
            if args.len() < 1 {
                Err(EvalError::IfNoConditionProvided)
            } else if args.len() < 2 {
                Err(EvalError::IfNoTrueBrach)
            } else {
                let condition = eval(&args[0], env.clone(), ctx)?;
                if condition.is_truthy() {
                    Ok(EvalStep::TailCall(args[1].clone(), env.clone()))
                } else {
                    if args.len() > 2 {
                        Ok(EvalStep::TailCall(args[2].clone(), env.clone()))
                    } else {
                        Ok(EvalStep::Done(LispExp::symbol("nil".into())))
                    }
                }
            }
        }

        "while" => {
            if args.is_empty() {
                return Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                });
            }

            let condition = &args[0];
            let body = &args[1..];

            // Every loop-body form may yield; the condition cannot record a continuation.
            if eval(condition, env.clone(), ctx)?.is_nil() {
                return Ok(EvalStep::Done(LispExp::nil()));
            }
            if !permitted {
                // This loop has no resumable frame, so its body cannot yield.
                let mut last_result = LispExp::nil();
                loop {
                    for exp in body {
                        last_result = eval(exp, env.clone(), ctx)?;
                    }
                    if eval(condition, env.clone(), ctx)?.is_nil() {
                        return Ok(EvalStep::Done(last_result));
                    }
                }
            }
            Ok(EvalStep::Done(run_while(
                condition,
                body,
                0,
                &env,
                ctx,
                LispExp::nil(),
            )?))
        }

        "spawn" => {
            if args.is_empty() {
                return Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                });
            }

            let target_closure = eval(&args[0], env.clone(), ctx)?;

            if let LispExp::Lambda(lambda_data) = target_closure {
                let lambda_clone = lambda_data.clone();
                let mut thread_ctx = ctx.clone();

                std::thread::spawn(move || {
                    thread_ctx.begin_thread_evaluation();
                    let thread_frame = Env::new_child(&lambda_clone.env);
                    for exp in &lambda_clone.body {
                        if let Err(err) = eval(exp, thread_frame.clone(), &mut thread_ctx) {
                            thread_ctx.log_diagnostic(&format!("[LISP thread] {err:?}"));
                            break;
                        }
                    }
                });

                Ok(EvalStep::Done(LispExp::form(vec![])))
            } else {
                Err(EvalError::WrongArgumentType {
                    expected: "Lambda".into(),
                    got: target_closure.clone(),
                })
            }
        }

        "yield" => {
            // Reject unsupported yields before evaluating their argument to avoid partial effects.
            if !permitted {
                return Err(EvalError::YieldNotAllowed);
            }
            let value = match args.first() {
                Some(form) => eval(form, env, ctx)?,
                None => LispExp::nil(),
            };
            Err(EvalError::Yielded {
                value,
                frames: Vec::new(),
            })
        }

        "fiber" => {
            if args.is_empty() {
                return Ok(EvalStep::Done(LispExp::fiber(FiberState {
                    pending: Vec::new(),
                    is_done: true,
                })));
            }
            // Capture the starting position and lexical environment in the same frame.
            Ok(EvalStep::Done(LispExp::fiber(FiberState {
                pending: vec![Frame::Body {
                    forms: args.to_vec(),
                    from: 0,
                    env: Env::new_child(&env),
                }],
                is_done: false,
            })))
        }

        "setq" => {
            if args.len() < 2 || args.len() % 2 != 0 {
                return Err(EvalError::SetqWrongNumberOfArgs(args.len()));
            }
            let mut is_symbol = true;
            let mut list_var_name = String::from("unreachable");
            let mut value = LispExp::symbol("nil".into());
            for arg in args {
                if is_symbol {
                    if let LispExp::Symbol(var_name) = arg {
                        list_var_name = var_name.to_string();
                        is_symbol = false;
                    } else {
                        return Err(EvalError::SetqSymbolRequired);
                    }
                } else {
                    value = eval(arg, env.clone(), ctx)?;
                    if !env.update_variable(&list_var_name, value.clone()) {
                        env.set_variable(list_var_name.clone(), value.clone());
                    }
                    is_symbol = true;
                }
            }
            Ok(EvalStep::Done(value))
        }

        "defun" => {
            if args.len() < 3 {
                return Err(EvalError::DefunNotCorrectExpression);
            }
            if let LispExp::Symbol(func_name) = &args[0] {
                let mut body_index = 2;
                let mut doc = None;
                let (params, optionals, rest) = if let LispExp::Form(params_list) = &args[1] {
                    parse_lambda_params(params_list)?
                } else {
                    return Err(EvalError::DefunParamsAreNotAList);
                };

                if let LispExp::String(doc_string) = &args[2]
                    && args.len() > 3
                {
                    doc = Some(Arc::new(doc_string.to_string()));
                    body_index = 3;
                }

                let lambda = Lambda {
                    params,
                    optionals,
                    rest,
                    body: args[body_index..].to_vec(),
                    env: env.clone(),
                    doc,
                };
                env.set_function(func_name.to_string(), LispExp::lambda(lambda));

                Ok(EvalStep::Done(LispExp::symbol(func_name.to_string())))
            } else {
                Err(EvalError::DefunNameMustBeASymbol)
            }
        }

        "lambda" => {
            if args.is_empty() {
                return Err(EvalError::DefunNotCorrectExpression);
            }

            let (params, optionals, rest) = if let LispExp::Form(params_list) = &args[0] {
                parse_lambda_params(params_list)?
            } else {
                return Err(EvalError::DefunParamsAreNotAList);
            };

            let body = args[1..].to_vec();

            Ok(EvalStep::Done(LispExp::lambda(Lambda {
                params,
                optionals,
                rest,
                body,
                env: env.clone(),
                doc: None,
            })))
        }

        "prog1" => {
            if args.is_empty() {
                Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                })
            } else {
                let first = eval(&args[0], env.clone(), ctx)?;
                for e in &args[1..] {
                    eval(e, env.clone(), ctx)?;
                }
                Ok(EvalStep::Done(first))
            }
        }

        "prog2" => {
            if args.len() < 2 {
                Err(EvalError::WrongNumberOfArguments {
                    expected: 2,
                    got: args.len(),
                })
            } else {
                eval(&args[0], env.clone(), ctx)?;
                let second = eval(&args[1], env.clone(), ctx)?;
                for e in &args[2..] {
                    eval(e, env.clone(), ctx)?;
                }
                Ok(EvalStep::Done(second))
            }
        }

        "progn" => eval_body_step(args, env, ctx, permitted),

        "let" => {
            if args.is_empty() {
                return Err(EvalError::LetNoBindingsProvided);
            }

            let let_env = Env::new_child(&env);
            if let LispExp::Form(bindings) = &args[0] {
                for (i, binding) in bindings.iter().enumerate() {
                    let (name, value_form) = parse_let_binding(binding, i)?;
                    let val = match value_form {
                        Some(value_form) => eval(&value_form, env.clone(), ctx)?,
                        None => LispExp::nil(),
                    };
                    let_env.set_variable(name, val);
                }
            } else if !args[0].is_nil() {
                return Err(EvalError::LetUnvalidBindingList);
            }

            eval_body_step(&args[1..], let_env, ctx, permitted)
        }

        // `let*` evaluates each initializer with the preceding bindings already visible.
        "let*" => {
            if args.is_empty() {
                Err(EvalError::LetNoBindingsProvided)
            } else {
                let let_env = Env::new_child(&env);
                if let LispExp::Form(bindings) = &args[0] {
                    for (i, binding) in bindings.iter().enumerate() {
                        let (name, value_form) = parse_let_binding(binding, i)?;
                        let val = match value_form {
                            Some(value_form) => eval(&value_form, let_env.clone(), ctx)?,
                            None => LispExp::nil(),
                        };
                        let_env.set_variable(name, val);
                    }
                } else if !args[0].is_nil() {
                    return Err(EvalError::LetUnvalidBindingList);
                }

                eval_body_step(&args[1..], let_env, ctx, permitted)
            }
        }

        "cond" => {
            for clause in args {
                let clause_list = if let LispExp::Form(clause_list) = clause {
                    clause_list
                } else {
                    return Err(EvalError::CondInvalidClause);
                };
                if clause_list.is_empty() {
                    return Err(EvalError::CondInvalidClause);
                }

                let test_val = eval(&clause_list[0], env.clone(), ctx)?;
                if test_val.is_truthy() {
                    let body = &clause_list[1..];
                    if body.is_empty() {
                        return Ok(EvalStep::Done(test_val));
                    }
                    return eval_body_step(body, env.clone(), ctx, permitted);
                }
            }
            Ok(EvalStep::Done(LispExp::nil()))
        }

        "and" => {
            if args.is_empty() {
                return Ok(EvalStep::Done(LispExp::t()));
            }
            for arg in &args[0..args.len() - 1] {
                if eval(arg, env.clone(), ctx)?.is_nil() {
                    return Ok(EvalStep::Done(LispExp::nil()));
                }
            }
            Ok(EvalStep::TailCall(
                args.last()
                    .expect("Failed to get the last and expression")
                    .clone(),
                env.clone(),
            ))
        }

        "or" => {
            if args.is_empty() {
                return Ok(EvalStep::Done(LispExp::nil()));
            }
            for arg in &args[0..args.len() - 1] {
                let val = eval(arg, env.clone(), ctx)?;
                if val.is_truthy() {
                    return Ok(EvalStep::Done(val));
                }
            }
            Ok(EvalStep::TailCall(
                args.last()
                    .expect("Failed to get the last or expression")
                    .clone(),
                env.clone(),
            ))
        }

        "when" => {
            if args.is_empty() {
                Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                })
            } else {
                if eval(&args[0], env.clone(), ctx)?.is_nil() {
                    Ok(EvalStep::Done(LispExp::nil()))
                } else {
                    eval_body_step(&args[1..], env, ctx, permitted)
                }
            }
        }

        "unless" => {
            if args.is_empty() {
                Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                })
            } else {
                if eval(&args[0], env.clone(), ctx)?.is_truthy() {
                    Ok(EvalStep::Done(LispExp::nil()))
                } else {
                    eval_body_step(&args[1..], env, ctx, permitted)
                }
            }
        }

        "dolist" => {
            if args.is_empty() {
                return Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                });
            }
            let spec = if let LispExp::Form(spec) = &args[0] {
                spec
            } else {
                return Err(EvalError::DolistInvalidBinding);
            };

            if spec.len() < 2 || spec.len() > 3 {
                return Err(EvalError::DolistInvalidBinding);
            }
            let var_name = if let LispExp::Symbol(name) = &spec[0] {
                name.to_string()
            } else {
                return Err(EvalError::DolistInvalidBinding);
            };

            let list_val = eval(&spec[1], env.clone(), ctx)?;

            match &list_val {
                LispExp::Cons(_) => (),
                other => {
                    if other.is_nil() {
                        ()
                    } else {
                        return Err(EvalError::WrongArgumentType {
                            expected: "List".into(),
                            got: other.clone(),
                        });
                    }
                }
            };

            let loop_env = Env::new_child(&env);
            loop_env.set_variable(var_name.clone(), LispExp::nil());
            let body = &args[1..];

            for item in list_val.iter() {
                ctx.consume_fuel(1)?;

                loop_env.update_variable(&var_name, item);
                for e in body {
                    eval(e, loop_env.clone(), ctx)?;
                }
            }

            if spec.len() == 3 {
                Ok(EvalStep::Done(eval(&spec[2], loop_env, ctx)?))
            } else {
                Ok(EvalStep::Done(LispExp::nil()))
            }
        }

        "dotimes" => {
            if args.is_empty() {
                return Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                });
            }
            let spec = if let LispExp::Form(spec) = &args[0] {
                spec
            } else {
                return Err(EvalError::DotimesInvalidBinding);
            };
            if spec.len() < 2 || spec.len() > 3 {
                return Err(EvalError::DotimesInvalidBinding);
            }
            let var_name = if let LispExp::Symbol(name) = &spec[0] {
                name.to_string()
            } else {
                return Err(EvalError::DotimesInvalidBinding);
            };

            let count_val = eval(&spec[1], env.clone(), ctx)?;
            let count = if let LispExp::Number(n) = count_val {
                n as i64
            } else {
                return Err(EvalError::WrongArgumentType {
                    expected: "Number".into(),
                    got: count_val.clone(),
                });
            };

            let loop_env = Env::new_child(&env);
            loop_env.set_variable(var_name.clone(), LispExp::number(0.0));
            let body = &args[1..];
            let mut i = 0;
            while i < count {
                ctx.consume_fuel(1)?;
                loop_env.update_variable(&var_name, LispExp::number(i as f64));
                for e in body {
                    eval(e, loop_env.clone(), ctx)?;
                }
                i += 1;
            }

            if spec.len() == 3 {
                Ok(EvalStep::Done(eval(&spec[2], loop_env, ctx)?))
            } else {
                Ok(EvalStep::Done(LispExp::nil()))
            }
        }

        // `defvar` preserves existing values; `defconst` always initializes.
        // Explicit docstrings replace the global property even if the value is preserved;
        // omitting a docstring leaves the previous property intact.
        "defvar" | "defconst" => {
            if args.is_empty() {
                Err(EvalError::DefvarNameMustBeASymbol)
            } else {
                let name = if let LispExp::Symbol(name) = &args[0] {
                    name.to_string()
                } else {
                    return Err(EvalError::DefunNameMustBeASymbol);
                };
                // Validate the definition before storing a property or evaluating its initializer.
                if args.len() > 3 {
                    return Err(EvalError::WrongNumberOfArguments {
                        expected: 3,
                        got: args.len(),
                    });
                }
                if let Some(doc) = args.get(2) {
                    let LispExp::String(_) = doc else {
                        return Err(EvalError::DefvarDocMustBeAString);
                    };
                    env.put_property(&name, VARIABLE_DOCUMENTATION, doc.clone());
                }
                if args.len() < 2 {
                    Ok(EvalStep::Done(LispExp::symbol(name)))
                } else {
                    if symbol == "defconst" || env.get_variable(&name).is_none() {
                        let val = eval(&args[1], env.clone(), ctx)?;
                        env.set_variable(name.clone(), val);
                    }
                    Ok(EvalStep::Done(LispExp::symbol(name)))
                }
            }
        }

        "unwind-protect" => {
            if args.is_empty() {
                Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                })
            } else {
                let body_result = eval(&args[0], env.clone(), ctx);
                // Preserve the body's failure frames while discarding cleanup frames.
                let depth_after_body = ctx.call_frame_depth();

                let run_cleanups = || {
                    let mut cleanup_error = None;

                    for cleanup in &args[1..] {
                        if let Err(err) = eval(cleanup, env.clone(), ctx) {
                            if cleanup_error.is_none() {
                                cleanup_error = Some(err);
                            }
                        }
                    }

                    cleanup_error
                };

                let cleanup_error = if args.len() > 1 {
                    ctx.with_unwind(run_cleanups)
                } else {
                    run_cleanups()
                };
                
                ctx.truncate_call_frames(depth_after_body);

                // Preserve the body's error if cleanup also fails; log the cleanup error.
                match (body_result, cleanup_error) {
                    (Err(body), Some(cleanup)) => {
                        ctx.log_diagnostic(&format!(
                            "[WARNING] unwind-protect cleanup also failed: {cleanup:?}"
                        ));
                        Err(body)
                    }
                    (Err(body), None) => Err(body),
                    (Ok(_), Some(cleanup)) => Err(cleanup),
                    (Ok(value), None) => Ok(EvalStep::Done(value)),
                }
            }
        }

        // Keep the protected body inside this Rust frame so throws can reach its catch.
        "catch" => {
            if args.is_empty() {
                return Err(EvalError::WrongNumberOfArguments {
                    expected: 1,
                    got: 0,
                });
            }
            let tag = eval(&args[0], env.clone(), ctx)?;
            let depth_before = ctx.call_frame_depth();
            let mut result = LispExp::nil();
            for form in &args[1..] {
                // Evaluate inside this frame so it can catch throws;
                // a tail call would escape the catch.
                match eval(form, env.clone(), ctx) {
                    Ok(value) => result = value,
                    Err(EvalError::Throw { tag: thrown, value }) => {
                        if thrown == tag {
                            ctx.truncate_call_frames(depth_before);
                            return Ok(EvalStep::Done(value));
                        }
                        return Err(EvalError::Throw { tag: thrown, value });
                    }
                    Err(err) => return Err(err),
                }
            }
            Ok(EvalStep::Done(result))
        }

        // Each handler is `(CONDITION BODY...)`; bind non-nil VAR to `(CONDITION . DATA)`.
        "condition-case" => {
            if args.len() < 2 {
                return Err(EvalError::WrongNumberOfArguments {
                    expected: 2,
                    got: args.len(),
                });
            }
            let var = match &args[0] {
                other if other.is_nil() => None,
                LispExp::Symbol(name) => Some(name.to_string()),
                _ => return Err(EvalError::ConditionCaseInvalidVariable),
            };

            let depth_before = ctx.call_frame_depth();
            // The protected evaluation must remain inside this frame to reach its handlers.
            let err = match eval(&args[1], env.clone(), ctx) {
                Ok(value) => return Ok(EvalStep::Done(value)),
                // Throws bypass condition handlers and continue to the matching catch.
                Err(throw @ EvalError::Throw { .. }) => return Err(throw),
                Err(err) => err,
            };

            let symbol = error_symbol(&err);
            for handler in &args[2..] {
                let clause = match handler {
                    LispExp::Form(clause) if !clause.is_empty() => clause,
                    _ => return Err(EvalError::ConditionCaseInvalidHandler),
                };
                if !condition_matches(&clause[0], &symbol) {
                    continue;
                }

                // Discard the handled failure's frames before another evaluation can fail.
                ctx.truncate_call_frames(depth_before);

                let handler_env = Env::new_child(&env);
                if let Some(name) = &var {
                    handler_env.set_variable(
                        name.clone(),
                        LispExp::cons(symbol.clone(), error_data(&err)),
                    );
                }
                if clause.len() == 1 {
                    return Ok(EvalStep::Done(LispExp::nil()));
                }
                for form in &clause[1..clause.len() - 1] {
                    eval(form, handler_env.clone(), ctx)?;
                }
                // The handler runs outside protection, so its final form can be tail-called.
                return Ok(EvalStep::TailCall(
                    clause[clause.len() - 1].clone(),
                    handler_env,
                ));
            }
            Err(err)
        }

        // Bind unevaluated syntax; evaluate the expansion in the caller's environment.
        "defmacro" => {
            if args.len() < 2 {
                Err(EvalError::DefunNotCorrectExpression)
            } else {
                if let LispExp::Symbol(macro_name) = &args[0] {
                    if let LispExp::Form(params_list) = &args[1] {
                        let (params, optionals, rest) = parse_lambda_params(params_list)?;
                        let lambda = Lambda {
                            params,
                            optionals,
                            rest,
                            body: args[2..].to_vec(),
                            env: env.clone(),
                            doc: None,
                        };
                        env.set_macro(macro_name.to_string(), LispExp::lambda(lambda));
                        Ok(EvalStep::Done(LispExp::symbol(macro_name.to_string())))
                    } else {
                        Err(EvalError::DefunParamsAreNotAList)
                    }
                } else {
                    Err(EvalError::DefunNameMustBeASymbol)
                }
            }
        }

        // Backquote supports one nesting level; comma inserts a value and comma-at
        // splices a list into the template.
        "backquote" => {
            if args.len() != 1 {
                Err(EvalError::BackquoteNotOneArgument)
            } else {
                Ok(EvalStep::Done(eval_backquote(&args[0], env.clone(), ctx)?))
            }
        }

        _ => eval_macro_or_function_call_step(symbol, args, env, ctx, permitted),
    }
}

/// Parses a single `let`/`let*` binding, which is either `(SYMBOL
/// VALUE-FORM)` or a bare `SYMBOL` (which binds to `nil`).
/// Returns the bound name and the value-form to evaluate, if any.
fn parse_let_binding<T: LispContext>(
    binding: &LispExp<T>,
    index: usize,
) -> Result<(String, Option<LispExp<T>>), EvalError<T>> {
    match binding {
        LispExp::Form(pair) if pair.len() == 2 => {
            if let LispExp::Symbol(name) = &pair[0] {
                Ok((name.to_string(), Some(pair[1].clone())))
            } else {
                Err(EvalError::LetUnvalidBindingAt(index))
            }
        }
        LispExp::Symbol(name) => Ok((name.to_string(), None)),
        _ => Err(EvalError::LetUnvalidBindingAt(index)),
    }
}

/// Expands a backquoted template, substituting `(unquote X)` forms with the
/// evaluation of `X` and splicing the evaluated list produced by
/// `(unquote-splicing X)` forms into the surrounding list.
fn eval_backquote<T: LispContext>(
    exp: &LispExp<T>,
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    fn is_tagged<T: LispContext>(list: &[LispExp<T>], tag: &str) -> bool {
        list.len() == 2 && matches!(&list[0], LispExp::Symbol(s) if s.as_str() == tag)
    }

    match exp {
        LispExp::Form(list) => {
            if is_tagged(list, "unquote") {
                return eval(&list[1], env, ctx);
            }

            let mut result = Vec::with_capacity(list.len());
            for item in list.iter() {
                if let LispExp::Form(inner) = item {
                    if is_tagged(inner, "unquote-splicing") {
                        let spliced = eval(&inner[1], env.clone(), ctx)?;
                        match &spliced {
                            LispExp::Cons(_) => result.extend(spliced.iter()),
                            LispExp::Form(spliced_list) => {
                                result.extend(spliced_list.iter().cloned());
                            }
                            other => {
                                if !other.is_nil() {
                                    return Err(EvalError::WrongArgumentType {
                                        expected: "List".into(),
                                        got: other.clone(),
                                    });
                                }
                            }
                        }
                        continue;
                    }
                }
                result.push(eval_backquote(item, env.clone(), ctx)?);
            }
            Ok(LispExp::proper_list(result))
        }
        LispExp::Vector(vec) => {
            let mut result = Vec::with_capacity(vec.len());
            for item in vec.iter() {
                result.push(eval_backquote(item, env.clone(), ctx)?);
            }
            Ok(LispExp::vec(result))
        }
        _ => Ok(exp.clone()),
    }
}

/// Dispatches a call whose head is a bare symbol: if the symbol names a
/// macro, expand it (with its arguments left unevaluated) and evaluate the
/// expansion in the *calling* environment; otherwise fall back to a normal
/// function call.
fn eval_macro_or_function_call_step<T: LispContext>(
    symbol: &str,
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
    permitted: bool,
) -> Result<EvalStep<T>, EvalError<T>> {
    if let Some(LispExp::Lambda(macro_lambda)) = env.get_macro(symbol) {
        let expand_frame = Env::new_child(&macro_lambda.env);
        bind_lambda_args(&macro_lambda, args, &expand_frame)?;

        let mut expansion = LispExp::nil();
        for form in &macro_lambda.body {
            expansion = eval(form, expand_frame.clone(), ctx)?;
        }

        Ok(EvalStep::TailCall(expansion, env))
    } else {
        eval_function_call_step(symbol, args, env, ctx, permitted)
    }
}

fn eval_function_call_step<T: LispContext>(
    symbol: &str,
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
    permitted: bool,
) -> Result<EvalStep<T>, EvalError<T>> {
    let mut evaled_args = Vec::new();
    for arg in args {
        evaled_args.push(eval(arg, env.clone(), ctx)?);
    }

    if let Some(func) = env.get_function(symbol) {
        ctx.push_call_frame(symbol);
        if let LispExp::Lambda(lambda) = func {
            let call_frame = Env::new_child(&lambda.env);
            bind_lambda_args(&lambda, &evaled_args, &call_frame)?;

            let step = eval_body_step(&lambda.body, call_frame, ctx, permitted);
            pop_unless_failed(ctx, &step);
            step
        } else if let LispExp::Primitive { pointer, doc: _ } = func {
            let result = pointer(&evaled_args[..], env.clone(), ctx)?;
            ctx.pop_call_frame();
            Ok(EvalStep::Done(result))
        } else {
            Err(EvalError::UncorrectFunctionDefinition)
        }
    } else {
        Err(EvalError::UndefinedFunction(symbol.into()))
    }
}
