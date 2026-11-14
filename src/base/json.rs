//! JSON primitives over the hand-written reader and writer in `json.rs`.
//! Both directions charge one fuel unit per byte of JSON text.
use super::*;
use crate::{parse_json, write_json};

const JSON_PARSE_STRING_DOC: &str = "(json-parse-string STRING): Parse the JSON \
                 value in STRING. Objects become maps, arrays vectors; true is \
                 t, while false and null are the symbols false and null, never \
                 nil. Numbers are floats; a repeated object key keeps its last \
                 value. Only whitespace may surround the value.\n\n\
                 Signals json-parse-error with data (MESSAGE LINE COLUMN \
                 POSITION): LINE and COLUMN count from 1, POSITION is the \
                 zero-based character offset, all pointing at the fault.\n\n\
                 Example:\n\
                 (json-parse-string \"{\\\"a\\\": [1, true, null]}\") => {a [1 t null]}\n\
                 (json-parse-string \"false\")                    => false";

fn primitive_json_parse_string<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let text = expect_string(&args[0])?;
    charge(ctx, text.len())?;
    parse_json(&text).map_err(|error| EvalError::Signal {
        symbol: LispExp::symbol("json-parse-error".into()),
        data: LispExp::proper_list(vec![
            LispExp::string(error.message),
            LispExp::number(error.line as f64),
            LispExp::number(error.column as f64),
            LispExp::number(error.position as f64),
        ]),
    })
}

const JSON_SERIALIZE_DOC: &str = "(json-serialize OBJECT): Return OBJECT as compact \
                 JSON text. Maps become objects with their keys sorted, vectors \
                 arrays; t, false and null become true, false and null; \
                 strings and finite numbers keep their value.\n\n\
                 Signals wrong-type-argument with data (json-value-p PART) for \
                 the first PART with no JSON meaning: nil and other symbols, \
                 lists, infinities and NaN, functions and other runtime \
                 objects. Write false or null where nil would be ambiguous, \
                 and a vector for an array.\n\n\
                 Example:\n\
                 (json-serialize {b [1 2.5] a \"x\"}) => \"{\\\"a\\\":\\\"x\\\",\\\"b\\\":[1,2.5]}\"\n\
                 (json-serialize '[t false null]) => \"[true,false,null]\"";

fn primitive_json_serialize<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let text = write_json(&args[0]).map_err(|part| EvalError::Signal {
        symbol: LispExp::symbol("wrong-type-argument".into()),
        data: LispExp::proper_list(vec![LispExp::symbol("json-value-p".into()), part]),
    })?;
    charge(ctx, text.len())?;
    Ok(LispExp::string(text))
}

/// Register this module's primitives and Lisp docstrings.
pub(super) fn install<T: LispContext>(into: &Registry<T>) {
    into.function(
        "json-parse-string",
        primitive_json_parse_string,
        JSON_PARSE_STRING_DOC,
    );
    into.function(
        "json-serialize",
        primitive_json_serialize,
        JSON_SERIALIZE_DOC,
    );
}
