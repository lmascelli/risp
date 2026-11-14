//! Reading data without evaluating it, and writing it back readably. Both
//! directions charge one fuel unit per byte of text.
use super::*;
use crate::{ParserError, Unreadable, write_readable};

/// Signal CONDITION with the reader's error as a one-string data list.
fn read_error<T: LispContext>(condition: &str, error: &ParserError) -> EvalError<T> {
    EvalError::Signal {
        symbol: LispExp::symbol(condition.into()),
        data: LispExp::proper_list(vec![LispExp::string(format!("{error:?}"))]),
    }
}

/// Map a reader failure to Emacs's conditions: input that stops short is
/// `end-of-file`, anything else malformed is `invalid-read-syntax`.
fn reader_condition<T: LispContext>(error: &ParserError) -> EvalError<T> {
    match error {
        ParserError::VoidExp
        | ParserError::UnclosedList
        | ParserError::UnclosedVector
        | ParserError::UnclosedMap
        | ParserError::UnclosedString => read_error("end-of-file", error),
        other => read_error("invalid-read-syntax", other),
    }
}

/// Signal `wrong-type-argument` with data `(readablep OFFENDING)`.
fn unreadable<T: LispContext>(why: Unreadable<T>, whole: &LispExp<T>) -> EvalError<T> {
    let offending = match why {
        Unreadable::Value(part) => part,
        Unreadable::TooDeep => whole.clone(),
    };
    EvalError::Signal {
        symbol: LispExp::symbol("wrong-type-argument".into()),
        data: LispExp::proper_list(vec![LispExp::symbol("readablep".into()), offending]),
    }
}

const READ_DOC: &str = "(read STRING): Read the one value written in STRING and \
                 return it without evaluating it. Lists come back as data, as \
                 if quoted; vectors and maps keep their elements unevaluated. \
                 Comments and surrounding whitespace are ignored. Unlike \
                 Emacs, STRING must hold exactly one value.\n\n\
                 Signals end-of-file when STRING ends before a value is \
                 complete, and invalid-read-syntax for malformed text or a \
                 second value; the data is a list holding the reader's error \
                 as a string.\n\n\
                 Example:\n\
                 (read \"(+ 1 2)\")          => (+ 1 2)\n\
                 (read \"[a (b . c)]\")      => [a (b . c)]\n\
                 (car (read \"(a b)\"))      => a";

fn primitive_read<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let text = expect_string(&args[0])?;
    charge(ctx, text.len())?;
    let mut parser = Parser::for_data(&text);
    let value = parser.next().map_err(|error| reader_condition(&error))?;
    match parser.next::<T>() {
        Err(ParserError::VoidExp) => Ok(value),
        Ok(_) => Err(read_error(
            "invalid-read-syntax",
            &ParserError::UnexpectedToken("a second value".into()),
        )),
        Err(error) => Err(read_error("invalid-read-syntax", &error)),
    }
}

const PRIN1_TO_STRING_DOC: &str = "(prin1-to-string OBJECT): Return text that \
                 read turns back into a value equal to OBJECT. Strings are \
                 quoted and escaped, map keys are sorted, and dotted lists \
                 keep their dot.\n\n\
                 Signals wrong-type-argument with data (readablep PART) when \
                 PART of OBJECT has no read syntax: a function, atom, fiber \
                 or thread handle, a non-finite number, or a symbol whose \
                 name would not read back as itself. Containers nested more \
                 than 256 deep are refused the same way, with PART the whole \
                 OBJECT. Emacs would print such parts as #<...> instead.\n\n\
                 Example:\n\
                 (prin1-to-string '(a \"b\" [1 2.5]))  => \"(a \\\"b\\\" [1 2.5])\"\n\
                 (read (prin1-to-string {b 2 a 1}))   => {a 1 b 2}";

fn primitive_prin1_to_string<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let text = write_readable(&args[0]).map_err(|why| unreadable(why, &args[0]))?;
    charge(ctx, text.len())?;
    Ok(LispExp::string(text))
}

const READABLEP_DOC: &str = "(readablep OBJECT): Return the text prin1-to-string \
                 would give for OBJECT, or nil if OBJECT cannot be written so \
                 that read gives it back.\n\n\
                 Example:\n\
                 (readablep '(1 2))            => \"(1 2)\"\n\
                 (readablep (lambda (x) x))    => nil\n\
                 (readablep (intern \"a b\"))    => nil";

fn primitive_readablep<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    match write_readable(&args[0]) {
        Ok(text) => {
            charge(ctx, text.len())?;
            Ok(LispExp::string(text))
        }
        Err(_) => Ok(LispExp::nil()),
    }
}

/// Register this module's primitives and Lisp docstrings.
pub(super) fn install<T: LispContext>(into: &Registry<T>) {
    into.function("read", primitive_read, READ_DOC);
    into.function(
        "prin1-to-string",
        primitive_prin1_to_string,
        PRIN1_TO_STRING_DOC,
    );
    into.function("readablep", primitive_readablep, READABLEP_DOC);
}
