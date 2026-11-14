//! Vector and map primitives. Every update returns a new value: Risp data is
//! shared and immutable, so the argument is never changed in place.
//!
//! Map keys are strings; a symbol key stands for its name, as in the reader's
//! `{a 1}`. Traversals visit keys in sorted order and charge fuel per entry.
use super::*;
use std::collections::HashMap;

fn expect_vector<T: LispContext>(exp: &LispExp<T>) -> Result<&[LispExp<T>], EvalError<T>> {
    match exp {
        LispExp::Vector(items) => Ok(items.as_slice()),
        other => Err(EvalError::WrongArgumentType {
            expected: "Vector".into(),
            got: other.clone(),
        }),
    }
}

fn expect_map<T: LispContext>(
    exp: &LispExp<T>,
) -> Result<&HashMap<String, LispExp<T>>, EvalError<T>> {
    match exp {
        LispExp::Map(map) => Ok(map),
        other => Err(EvalError::WrongArgumentType {
            expected: "Map".into(),
            got: other.clone(),
        }),
    }
}

/// Read a map key: a string, or a symbol standing for its name.
fn expect_key<T: LispContext>(exp: &LispExp<T>) -> Result<String, EvalError<T>> {
    match exp {
        LispExp::String(s) | LispExp::Symbol(s) => Ok((**s).clone()),
        other => Err(EvalError::WrongArgumentType {
            expected: "String or symbol".into(),
            got: other.clone(),
        }),
    }
}

/// Resolve INDEX into VECTOR, signalling `args-out-of-range` as Emacs does.
fn vector_index<T: LispContext>(
    vector: &LispExp<T>,
    items: &[LispExp<T>],
    index: &LispExp<T>,
) -> Result<usize, EvalError<T>> {
    let n = expect_number(index)?;
    if n >= 0.0 && n.fract() == 0.0 && n < items.len() as f64 {
        Ok(n as usize)
    } else {
        Err(EvalError::Signal {
            symbol: LispExp::symbol("args-out-of-range".into()),
            data: LispExp::proper_list(vec![vector.clone(), index.clone()]),
        })
    }
}

/// The entries of MAP sorted by key, charging one fuel unit per entry.
fn sorted_entries<'a, T: LispContext>(
    map: &'a HashMap<String, LispExp<T>>,
    ctx: &T,
) -> Result<Vec<(&'a String, &'a LispExp<T>)>, EvalError<T>> {
    charge(ctx, map.len())?;
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
    Ok(entries)
}

fn arity_between<T: LispContext>(
    args: &[LispExp<T>],
    min: usize,
    max: usize,
) -> Result<(), EvalError<T>> {
    if args.len() < min || args.len() > max {
        return Err(EvalError::WrongNumberOfArguments {
            expected: if args.len() < min { min } else { max },
            got: args.len(),
        });
    }
    Ok(())
}

const VECTOR_DOC: &str = "(vector &rest OBJECTS): Return a new vector holding \
                 OBJECTS in order.\n\n\
                 Example:\n\
                 (vector 1 \"a\" 'b) => [1 \"a\" b]\n\
                 (vector)          => []";

fn primitive_vector<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    charge(ctx, args.len())?;
    Ok(LispExp::vec(args.to_vec()))
}

const MAKE_VECTOR_DOC: &str = "(make-vector LENGTH INIT): Return a new vector of \
                 LENGTH elements, each INIT. A LENGTH of zero or less gives the \
                 empty vector.\n\n\
                 Example:\n\
                 (make-vector 3 0) => [0 0 0]";

fn primitive_make_vector<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 2)?;
    let count = expect_number(&args[0])?;
    let count = if count.is_finite() {
        count.max(0.0) as usize
    } else {
        0
    };
    // Charge before allocating so the budget bounds the requested size.
    charge(ctx, count)?;
    let mut items = Vec::new();
    items.try_reserve_exact(count).map_err(|_| {
        EvalError::RuntimeMessage("make-vector: unable to allocate requested vector".into())
    })?;
    items.resize(count, args[1].clone());
    Ok(LispExp::vec(items))
}

const AREF_DOC: &str = "(aref VECTOR INDEX): Return the element of VECTOR at the \
                 zero-based INDEX. Signals args-out-of-range when INDEX is not \
                 a whole number inside VECTOR; see elt for a lenient read.\n\n\
                 Example:\n\
                 (aref '[a b c] 1) => b";

fn primitive_aref<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    _ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 2)?;
    let items = expect_vector(&args[0])?;
    let index = vector_index(&args[0], items, &args[1])?;
    Ok(items[index].clone())
}

const VECTOR_SET_DOC: &str = "(vector-set VECTOR INDEX VALUE): Return a copy of \
                 VECTOR whose element at INDEX is VALUE. VECTOR itself is not \
                 changed; Risp has no destructive aset. Signals \
                 args-out-of-range as aref does.\n\n\
                 Example:\n\
                 (setq v '[a b c])\n\
                 (vector-set v 1 'x) => [a x c]\n\
                 v                   => [a b c]";

fn primitive_vector_set<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 3)?;
    let items = expect_vector(&args[0])?;
    let index = vector_index(&args[0], items, &args[1])?;
    charge(ctx, items.len())?;
    let mut copy = items.to_vec();
    copy[index] = args[2].clone();
    Ok(LispExp::vec(copy))
}

const MAKE_MAP_DOC: &str = "(make-map &rest KEY VALUE...): Return a new map from \
                 alternating KEYs and VALUEs. A KEY is a string or a symbol, \
                 which stands for its name; a repeated KEY keeps its last \
                 VALUE. Literal {KEY VALUE ...} builds the same map.\n\n\
                 Example:\n\
                 (make-map 'a 1 \"b\" 2) => {a 1 b 2}\n\
                 (make-map)            => {}";

fn primitive_make_map<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    if !args.len().is_multiple_of(2) {
        return Err(EvalError::WrongNumberOfArguments {
            expected: args.len() + 1,
            got: args.len(),
        });
    }
    charge(ctx, args.len() / 2)?;
    let mut map = HashMap::with_capacity(args.len() / 2);
    for pair in args.chunks_exact(2) {
        map.insert(expect_key(&pair[0])?, pair[1].clone());
    }
    Ok(LispExp::map(map))
}

const MAPP_DOC: &str = "(mapp OBJECT): Return t if OBJECT is a map, nil \
                 otherwise. Unlike Emacs's map.el, lists are not maps.\n\n\
                 Example:\n\
                 (mapp {a 1})     => t\n\
                 (mapp '((a . 1))) => nil";

predicate!(primitive_mapp, LispExp::Map(_));

const MAP_ELT_DOC: &str = "(map-elt MAP KEY &optional DEFAULT): Return the value \
                 MAP holds for KEY, or DEFAULT (nil when omitted) if KEY is \
                 absent. KEY is a string or a symbol.\n\n\
                 Example:\n\
                 (map-elt {a 1} 'a)       => 1\n\
                 (map-elt {a 1} \"a\")     => 1\n\
                 (map-elt {a 1} 'b 'none) => none";

fn primitive_map_elt<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    _ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    arity_between(args, 2, 3)?;
    let map = expect_map(&args[0])?;
    let key = expect_key(&args[1])?;
    Ok(map
        .get(&key)
        .cloned()
        .unwrap_or_else(|| args.get(2).cloned().unwrap_or_else(LispExp::nil)))
}

const MAP_CONTAINS_KEY_DOC: &str = "(map-contains-key MAP KEY): Return t if MAP \
                 has an entry for KEY, even one whose value is nil.\n\n\
                 Example:\n\
                 (map-contains-key {a nil} 'a) => t\n\
                 (map-contains-key {a nil} 'b) => nil";

fn primitive_map_contains_key<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    _ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 2)?;
    let map = expect_map(&args[0])?;
    Ok(LispExp::boolean(map.contains_key(&expect_key(&args[1])?)))
}

const MAP_INSERT_DOC: &str = "(map-insert MAP KEY VALUE): Return a copy of MAP \
                 in which KEY holds VALUE. MAP itself is not changed.\n\n\
                 Example:\n\
                 (setq m {a 1})\n\
                 (map-insert m 'b 2) => {a 1 b 2}\n\
                 m                   => {a 1}";

fn primitive_map_insert<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 3)?;
    let map = expect_map(&args[0])?;
    let key = expect_key(&args[1])?;
    charge(ctx, map.len())?;
    let mut copy = map.clone();
    copy.insert(key, args[2].clone());
    Ok(LispExp::map(copy))
}

const MAP_DELETE_DOC: &str = "(map-delete MAP KEY): Return a copy of MAP without \
                 KEY, or an equal map if KEY is absent. MAP itself is not \
                 changed, unlike map.el's destructive map-delete.\n\n\
                 Example:\n\
                 (map-delete {a 1 b 2} 'a) => {b 2}";

fn primitive_map_delete<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 2)?;
    let map = expect_map(&args[0])?;
    let key = expect_key(&args[1])?;
    charge(ctx, map.len())?;
    let mut copy = map.clone();
    copy.remove(&key);
    Ok(LispExp::map(copy))
}

const MAP_KEYS_DOC: &str = "(map-keys MAP): Return the keys of MAP as a list of \
                 strings in sorted order.\n\n\
                 Example:\n\
                 (map-keys {b 2 a 1}) => (\"a\" \"b\")";

fn primitive_map_keys<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let entries = sorted_entries(expect_map(&args[0])?, ctx)?;
    Ok(LispExp::proper_list(
        entries
            .into_iter()
            .map(|(key, _)| LispExp::string(key.clone()))
            .collect(),
    ))
}

const MAP_VALUES_DOC: &str = "(map-values MAP): Return the values of MAP as a \
                 list, in the order of their sorted keys.\n\n\
                 Example:\n\
                 (map-values {b 2 a 1}) => (1 2)";

fn primitive_map_values<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let entries = sorted_entries(expect_map(&args[0])?, ctx)?;
    Ok(LispExp::proper_list(
        entries
            .into_iter()
            .map(|(_, value)| value.clone())
            .collect(),
    ))
}

const MAP_PAIRS_DOC: &str = "(map-pairs MAP): Return MAP as an association list \
                 of (KEY . VALUE) cells, KEY a string, in sorted key order.\n\n\
                 Example:\n\
                 (map-pairs {b 2 a 1}) => ((\"a\" . 1) (\"b\" . 2))";

fn primitive_map_pairs<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    let entries = sorted_entries(expect_map(&args[0])?, ctx)?;
    Ok(LispExp::proper_list(
        entries
            .into_iter()
            .map(|(key, value)| LispExp::cons(LispExp::string(key.clone()), value.clone()))
            .collect(),
    ))
}

const MAP_LENGTH_DOC: &str = "(map-length MAP): Return the number of entries in \
                 MAP.\n\n\
                 Example:\n\
                 (map-length {a 1 b 2}) => 2";

fn primitive_map_length<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    _ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 1)?;
    Ok(LispExp::number(expect_map(&args[0])?.len() as f64))
}

const MAP_DO_DOC: &str = "(map-do FUNCTION MAP): Call FUNCTION with each KEY and \
                 VALUE of MAP, in sorted key order, for its side effects. KEY \
                 is passed as a string. Returns nil.\n\n\
                 Example:\n\
                 (let ((sum 0))\n\
                   (map-do (lambda (k v) (setq sum (+ sum v))) {a 1 b 2})\n\
                   sum) => 3";

fn primitive_map_do<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 2)?;
    for (key, value) in sorted_entries(expect_map(&args[1])?, ctx)? {
        let call_args = [LispExp::string(key.clone()), value.clone()];
        call_callable(&args[0], &call_args, env.clone(), ctx)?;
    }
    Ok(LispExp::nil())
}

const MAP_APPLY_DOC: &str = "(map-apply FUNCTION MAP): Call FUNCTION with each KEY \
                 and VALUE of MAP, in sorted key order, and return a list of \
                 the results. KEY is passed as a string.\n\n\
                 Example:\n\
                 (map-apply (lambda (k v) (concat k \"=\" (number-to-string v))) {b 2 a 1})\n\
                 => (\"a=1\" \"b=2\")";

fn primitive_map_apply<T: LispContext>(
    args: &[LispExp<T>],
    env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    exact_arity(args, 2)?;
    let entries = sorted_entries(expect_map(&args[1])?, ctx)?;
    let mut results = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let call_args = [LispExp::string(key.clone()), value.clone()];
        results.push(call_callable(&args[0], &call_args, env.clone(), ctx)?);
    }
    Ok(LispExp::proper_list(results))
}

/// Register this module's primitives and Lisp docstrings.
pub(super) fn install<T: LispContext>(into: &Registry<T>) {
    into.function("vector", primitive_vector, VECTOR_DOC);
    into.function("make-vector", primitive_make_vector, MAKE_VECTOR_DOC);
    into.function("aref", primitive_aref, AREF_DOC);
    into.function("vector-set", primitive_vector_set, VECTOR_SET_DOC);
    into.function("make-map", primitive_make_map, MAKE_MAP_DOC);
    into.function("mapp", primitive_mapp, MAPP_DOC);
    into.function("map-elt", primitive_map_elt, MAP_ELT_DOC);
    into.function(
        "map-contains-key",
        primitive_map_contains_key,
        MAP_CONTAINS_KEY_DOC,
    );
    into.function("map-insert", primitive_map_insert, MAP_INSERT_DOC);
    into.function("map-delete", primitive_map_delete, MAP_DELETE_DOC);
    into.function("map-keys", primitive_map_keys, MAP_KEYS_DOC);
    into.function("map-values", primitive_map_values, MAP_VALUES_DOC);
    into.function("map-pairs", primitive_map_pairs, MAP_PAIRS_DOC);
    into.function("map-length", primitive_map_length, MAP_LENGTH_DOC);
    into.function("map-do", primitive_map_do, MAP_DO_DOC);
    into.function("map-apply", primitive_map_apply, MAP_APPLY_DOC);
}
