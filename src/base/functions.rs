//! Function invocation, evaluation, non-local exits, and documentation lookup.
use super::*;

const FUNCALL_DOC: &str = "(funcall FUNCTION &rest ARGS): Call FUNCTION with ARGS. \
                 FUNCTION may be a lambda, a primitive, or a symbol naming a \
                 function in the function namespace -- same symbol \
                 resolution as `apply`/`mapcar`.\n\n\
                 Example:\n\
                 (funcall (lambda (x y) (+ x y)) 2 3) => 5\n\
                 (funcall 'car '(1 2))                => 1";

fn primitive_funcall<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    some_arguments(args)?;

    let func_obj = &args[0];
    let func_args = &args[1..];

    call_callable(func_obj, func_args, env, ctx)
}

const EVAL_DOC: &str = "(eval FORM): Evaluate FORM -- an already-parsed \
                 Lisp expression, such as a quoted list -- in the current \
                 environment and return the result. Any arguments after \
                 FORM are ignored; unlike real Emacs Lisp's `eval`, there \
                 is no optional LEXICAL argument.\n\n\
                 Example:\n\
                 (eval '(+ 1 2)) => 3\n\
                 (setq form '(* 2 3)) (eval form) => 6";

fn primitive_eval<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    if args.is_empty() {
        Err(EvalError::WrongNumberOfArguments {
            expected: 1,
            got: 0,
        })
    } else {
        eval(&args[0], env.clone(), ctx)
    }
}

const EVAL_STRING_DOC: &str = "(eval-string STRING): Parse STRING as Lisp \
                 source and evaluate the first top-level expression it \
                 contains in the current environment, returning the \
                 result. Only the first expression is evaluated -- wrap \
                 multiple forms in an explicit (progn ...) if STRING needs \
                 to contain more than one. Not a standard Elisp primitive; \
                 real Emacs Lisp achieves the same effect with \
                 (eval (read STRING)).\n\n\
                 Example:\n\
                 (eval-string \"(+ 1 2)\") => 3\n\
                 (eval-string \"(progn (setq x 10) (* x 2))\") => 20";

fn primitive_eval_string<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    if args.is_empty() {
        Err(EvalError::WrongNumberOfArguments {
            expected: 1,
            got: 0,
        })
    } else {
        if let LispExp::String(source) = &args[0] {
            let mut parser = Parser::new(source);
            match parser.next() {
                Ok(ast) => eval(&ast, env.clone(), ctx),
                Err(parse_error) => Err(EvalError::RuntimeMessage(format!(
                    "eval-string: Parser Error {:?}",
                    parse_error
                ))),
            }
        } else {
            Err(EvalError::WrongArgumentType {
                expected: "String".into(),
                got: args[0].clone(),
            })
        }
    }
}

const EVAL_STRING_SAFE_DOC: &str = "(eval-string-safe STRING): Like `eval-string', but never \
                 propagates an error out of the call. Parses and evaluates \
                 STRING and returns a two-element list: (t RESULT) if \
                 parsing and evaluation both succeeded, or (nil DESCRIPTION) \
                 if either failed, where DESCRIPTION is a human-readable \
                 string describing what went wrong. Meant for interactive \
                 evaluation (a REPL, a minibuffer, ...) where a bad \
                 expression -- an unbound variable, a typo, wrong argument \
                 counts -- shouldn't abort whatever triggered the \
                 evaluation.\n\n\
                 Example:\n\
                 (eval-string-safe \"(+ 1 2)\") => (t 3)\n\
                 (eval-string-safe \"(1 2 3)\")  => (nil \"UnvalidFunctionCall\")";

fn primitive_eval_string_safe<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    if args.is_empty() {
        Err(EvalError::WrongNumberOfArguments {
            expected: 1,
            got: 0,
        })
    } else if let LispExp::String(source) = &args[0] {
        // The error is returned as data, so discard frames left by the failed evaluation.
        let depth_before = ctx.call_frame_depth();
        let mut parser = Parser::new(source);
        let result = match parser.next() {
            Ok(ast) => eval(&ast, env.clone(), ctx),
            Err(parse_error) => Err(EvalError::RuntimeMessage(format!(
                "Parser Error {:?}",
                parse_error
            ))),
        };
        ctx.truncate_call_frames(depth_before);
        Ok(match result {
            Ok(value) => LispExp::proper_list(vec![LispExp::t(), value]),
            Err(err) => {
                LispExp::proper_list(vec![LispExp::nil(), LispExp::string(format!("{:?}", err))])
            }
        })
    } else {
        Err(EvalError::WrongArgumentType {
            expected: "String".into(),
            got: args[0].clone(),
        })
    }
}

const FUNCTION_DOC_DOC: &str = "(function-doc SYMBOL): Return the documentation string of the \
                 function bound to SYMBOL, or \"Undocumented function\" if it \
                 has none. Returns nil and logs a diagnostic if SYMBOL names no \
                 function. Not a standard Elisp primitive -- the closest real \
                 Elisp equivalent is `documentation`.\n\n\
                 Example:\n\
                 (function-doc 'car) => \"(car LIST): Return the first \
                 element of LIST, or nil if LIST is nil.\"\n\
                 (function-doc 'no-such-fn) => nil";

fn primitive_function_doc<T: LispContext>(
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
        err
    } else {
        if let LispExp::Symbol(func_name) = &args[0] {
            if let Some(func) = env.get_function(&func_name) {
                match func {
                    LispExp::Lambda(lambda) => {
                        let doc = if let Some(doc) = &lambda.doc {
                            doc.to_string()
                        } else {
                            "Undocumented function".into()
                        };
                        Ok(LispExp::string(doc))
                    }
                    LispExp::Primitive { pointer: _, doc } => Ok(LispExp::string(doc.to_string())),
                    _ => unreachable!(),
                }
            } else {
                ctx.log_diagnostic(&format!(
                    "{} function is not present in the environment",
                    func_name.as_str()
                ));
                Ok(nil!())
            }
        } else {
            let err = Err(EvalError::WrongArgumentType {
                expected: "Symbol".into(),
                got: args[0].clone(),
            });
            ctx.log_diagnostic(&format!("{err:?}"));
            err
        }
    }
}

const VARIABLE_DOC_DOC: &str = "(variable-doc SYMBOL): Return the documentation string \
                 `defvar\' or `defconst\' was given for SYMBOL, or \"Undocumented variable\" if it \
                 is bound and has none. Returns nil for a name that is neither bound nor \
                 documented.\n\n\
                 The documentation belongs to the symbol rather than to the binding, so a `let\' \
                 that shadows a variable does not shadow what it means, and a variable can be \
                 documented before it has any value at all.\n\n\
                 The same string is readable as an ordinary property: (get SYMBOL \
                 \'variable-documentation).\n\n\
                 Example:\n\
                 (defvar fill-column 70 \"Where lines are wrapped.\")\n\
                 (variable-doc \'fill-column) => \"Where lines are wrapped.\"";

fn primitive_variable_doc<T: LispContext>(
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
    // Documentation belongs to the symbol, even when it has no variable binding.
    if let Some(doc) = env.get_property(name, VARIABLE_DOCUMENTATION) {
        return Ok(doc);
    }
    if env.get_variable(name).is_some() {
        return Ok(LispExp::string("Undocumented variable".to_string()));
    }
    ctx.log_diagnostic(&format!(
        "{} names neither a bound nor a documented variable",
        name.as_str()
    ));
    Ok(nil!())
}

const THROW_DOC: &str = "(throw TAG VALUE): Jump to the innermost enclosing \
                 `catch' whose tag equals TAG, making that `catch' return \
                 VALUE. Signals an error if no such `catch' is active.\n\n\
                 A `catch' with a different tag does not intercept it, an \
                 `unwind-protect' in between still runs its cleanups, and a \
                 `condition-case' deliberately does not catch it -- a throw is \
                 a control transfer, not a failure.\n\n\
                 Example:\n\
                 (catch 'done (dolist (x '(1 2 3)) (if (= x 2) (throw 'done x))))\n\
                 => 2";

raise!(primitive_throw, Throw { tag, value });

const SIGNAL_DOC: &str = "(signal SYMBOL DATA): Raise the error condition SYMBOL, carrying \
                 DATA. A `condition-case' handler for SYMBOL (or for `error', \
                 which matches everything) receives (SYMBOL . DATA) in its \
                 variable.\n\n\
                 Example:\n\
                 (condition-case e (signal 'my-error '(1 2)) (my-error (cdr e)))\n\
                 => (1 2)";

raise!(primitive_signal, Signal { symbol, data });

/// Register this module's primitives and Lisp docstrings.
pub(super) fn install<T: LispContext>(into: &Registry<T>) {
    into.function("funcall", primitive_funcall, FUNCALL_DOC);
    into.function("eval", primitive_eval, EVAL_DOC);
    into.function("throw", primitive_throw, THROW_DOC);
    into.function("signal", primitive_signal, SIGNAL_DOC);
    into.function("eval-string", primitive_eval_string, EVAL_STRING_DOC);
    into.function(
        "eval-string-safe",
        primitive_eval_string_safe,
        EVAL_STRING_SAFE_DOC,
    );
    into.function("function-doc", primitive_function_doc, FUNCTION_DOC_DOC);
    into.function("variable-doc", primitive_variable_doc, VARIABLE_DOC_DOC);
}
