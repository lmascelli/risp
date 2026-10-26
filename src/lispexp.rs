//! Lisp values, syntax forms, symbol interning, equality, and printing.

use super::{
    ConsCell, ConsIter, FiberState, Lambda, LispContext, LispMap, LispPrimitive, LispVec,
    SharedAtom, SharedFiber, SharedThreadHandle,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, OnceLock, RwLock},
};

/// Process-wide symbol names, interned independently of the host context.
/// Symbol properties live in environments because they contain context-specific values.
fn obarray() -> &'static RwLock<HashMap<String, Arc<String>>> {
    static OBARRAY: OnceLock<RwLock<HashMap<String, Arc<String>>>> = OnceLock::new();
    OBARRAY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Return the process-wide interned allocation for `name`.
pub fn intern(name: String) -> Arc<String> {
    if let Some(found) = obarray()
        .read()
        .expect("Failed to acquire read lock on the obarray")
        .get(&name)
    {
        return found.clone();
    }
    obarray()
        .write()
        .expect("Failed to acquire write lock on the obarray")
        .entry(name.clone())
        .or_insert_with(|| Arc::new(name))
        .clone()
}

/// Cache nil's interned name to avoid locking on repeated construction.
fn nil_name() -> &'static Arc<String> {
    static NIL: OnceLock<Arc<String>> = OnceLock::new();
    NIL.get_or_init(|| intern("nil".to_string()))
}

fn t_name() -> &'static Arc<String> {
    static T: OnceLock<Arc<String>> = OnceLock::new();
    T.get_or_init(|| intern("t".to_string()))
}

/// A Lisp value or reader syntax node. Clones share compound allocations.
///
/// Use data cons lists for runtime values and forms for executable syntax.
/// [`PartialEq`] compares lists structurally and closures by identity; Lisp
/// `eq` has separate semantics. [`std::fmt::Debug`] uses Lisp notation.
#[derive(Clone)]
pub enum LispExp<T: LispContext> {
    /// Vector-backed syntax used for evaluation. Use [`Self::proper_list`]
    /// for data returned to Lisp; an empty form is also treated as nil.
    Form(Arc<LispVec<T>>),

    /// Data cons cell; nil terminates a proper list, another value a dotted list.
    Cons(Arc<ConsCell<T>>),

    /// A vector of Lisp values.
    Vector(Arc<LispVec<T>>),
    /// A map keyed by strings or symbol names.
    Map(Arc<LispMap<T>>),
    /// An IEEE 754 double-precision number.
    Number(f64),
    /// An interned symbol name.
    Symbol(Arc<String>),
    /// An immutable UTF-8 string.
    String(Arc<String>),
    /// A lexical closure.
    Lambda(Arc<Lambda<T>>),
    /// A native callable and its Lisp documentation.
    Primitive {
        /// Native implementation invoked with evaluated arguments.
        pointer: LispPrimitive<T>,
        /// Lisp docstring; defaults to "No documentation provided." when absent.
        doc: Arc<Cow<'static, str>>,
    },
    /// A shared mutable value.
    Atom(SharedAtom<T>),
    /// A cooperative suspended computation.
    Fiber(SharedFiber<T>),
    ThreadHandle(SharedThreadHandle<T>),
}

/// Compare compound values using an explicit work stack.
/// Closures retain identity-based equality.
#[allow(unpredictable_function_pointer_comparisons)]
impl<T: LispContext> PartialEq for LispExp<T> {
    fn eq(&self, other: &Self) -> bool {
        use LispExp::*;

        let mut pending = Vec::new();
        let mut current = (self, other);

        loop {
            match current {
                (Cons(a), Cons(b)) => {
                    if !Arc::ptr_eq(a, b) {
                        pending.push((&a.cdr, &b.cdr));
                        current = (&a.car, &b.car);
                        continue;
                    }
                }

                (Form(a), Form(b)) | (Vector(a), Vector(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }

                    pending.extend(a.iter().zip(b.iter()).rev());
                }

                (Map(a), Map(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }

                    for (key, value) in a.iter() {
                        let Some(other_value) = b.get(key) else {
                            return false;
                        };

                        pending.push((value, other_value));
                    }
                }

                (Lambda(a), Lambda(b)) => {
                    if !Arc::ptr_eq(a, b) {
                        return false;
                    }
                }

                (Number(a), Number(b)) => {
                    if a != b {
                        return false;
                    }
                }

                (Symbol(a), Symbol(b)) | (String(a), String(b)) => {
                    if a != b {
                        return false;
                    }
                }

                (Atom(a), Atom(b)) => {
                    if a != b {
                        return false;
                    }
                }

                (Fiber(a), Fiber(b)) => {
                    if a != b {
                        return false;
                    }
                }

                (Primitive { pointer: a, .. }, Primitive { pointer: b, .. }) => {
                    if a != b {
                        return false;
                    }
                }

                _ => return false,
            }

            match pending.pop() {
                Some(pair) => current = pair,
                None => return true,
            }
        }
    }
}

/// Format values without recursively traversing compound values.
/// Runtime-only values retain their opaque representations.
impl<T: LispContext> std::fmt::Debug for LispExp<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        enum Task<'a, C: LispContext> {
            Value(&'a LispExp<C>),
            Text(&'static str),
            Tail(&'a LispExp<C>),
            Sequence(std::slice::Iter<'a, LispExp<C>>, bool),
            MapEntries(
                std::collections::hash_map::Iter<'a, String, LispExp<C>>,
                bool,
            ),
        }

        let mut pending = vec![Task::Value(self)];

        while let Some(task) = pending.pop() {
            match task {
                Task::Text(text) => f.write_str(text)?,

                Task::Sequence(mut items, first) => {
                    if let Some(item) = items.next() {
                        if !first {
                            f.write_str(" ")?;
                        }

                        pending.push(Task::Sequence(items, false));
                        pending.push(Task::Value(item));
                    }
                }

                Task::MapEntries(mut entries, first) => {
                    if let Some((key, value)) = entries.next() {
                        if !first {
                            f.write_str(" ")?;
                        }

                        write!(f, "{} ", key)?;

                        pending.push(Task::MapEntries(entries, false));
                        pending.push(Task::Value(value));
                    }
                }

                Task::Tail(value) => match value {
                    LispExp::Cons(cell) => {
                        f.write_str(" ")?;
                        pending.push(Task::Tail(&cell.cdr));
                        pending.push(Task::Value(&cell.car));
                    }
                    other if !other.is_nil() => {
                        f.write_str(" . ")?;
                        pending.push(Task::Value(other));
                    }
                    _ => {}
                },

                Task::Value(value) => match value {
                    LispExp::Form(items) => {
                        f.write_str("(")?;
                        pending.push(Task::Text(")"));
                        pending.push(Task::Sequence(items.iter(), true));
                    }

                    LispExp::Cons(cell) => {
                        f.write_str("(")?;
                        pending.push(Task::Text(")"));
                        pending.push(Task::Tail(&cell.cdr));
                        pending.push(Task::Value(&cell.car));
                    }

                    LispExp::Vector(items) => {
                        f.write_str("[")?;
                        pending.push(Task::Text("]"));
                        pending.push(Task::Sequence(items.iter(), true));
                    }

                    LispExp::Map(map) => {
                        f.write_str("{")?;
                        pending.push(Task::Text("}"));
                        pending.push(Task::MapEntries(map.iter(), true));
                    }

                    LispExp::Number(n) => {
                        if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                            write!(f, "{}", *n as i64)?;
                        } else {
                            write!(f, "{}", n)?;
                        }
                    }

                    LispExp::Symbol(s) => write!(f, "{}", s)?,

                    LispExp::String(s) => {
                        write!(f, "{:?}", s.as_str())?;
                    }

                    LispExp::Lambda(lambda) => {
                        f.write_str("#<lambda (")?;
                        let mut first = true;

                        for param in &lambda.params {
                            if !first {
                                f.write_str(" ")?;
                            }
                            first = false;
                            write!(f, "{}", param)?;
                        }

                        if !lambda.optionals.is_empty() {
                            if !first {
                                f.write_str(" ")?;
                            }
                            first = false;

                            f.write_str("&optional")?;
                            for param in &lambda.optionals {
                                write!(f, " {}", param)?;
                            }
                        }

                        if let Some(rest) = &lambda.rest {
                            if !first {
                                f.write_str(" ")?;
                            }
                            write!(f, "&rest {}", rest)?;
                        }

                        f.write_str(")>")?;
                    }

                    LispExp::Primitive { .. } => {
                        f.write_str("#<primitive>")?;
                    }

                    // Atoms may contain themselves or be locked by the caller.
                    LispExp::Atom(_) => f.write_str("#<atom>")?,

                    LispExp::Fiber(fiber) => match fiber.0.try_read() {
                        Ok(state) if state.status == crate::FiberStatus::Done => {
                            f.write_str("#<fiber done>")?;
                        }
                        Ok(state) if state.status == crate::FiberStatus::Running => {
                            f.write_str("#<fiber running>")?;
                        }
                        Ok(_) => f.write_str("#<fiber>")?,
                        Err(_) => f.write_str("#<fiber running>")?,
                    },

                    LispExp::ThreadHandle(_) => {
                        f.write_str("#t<thread-handle>")?;
                    }
                },
            }
        }

        Ok(())
    }
}

impl<T: LispContext> LispExp<T> {
    /// Intern a symbol name. Symbols other than `nil`, `t`, and keywords require lookup.
    pub fn symbol(value: String) -> LispExp<T> {
        LispExp::Symbol(intern(value))
    }

    /// Return the canonical empty list and false value, the symbol `nil`.
    pub fn nil() -> LispExp<T> {
        LispExp::Symbol(nil_name().clone())
    }

    /// Return the conventional true symbol `t`.
    pub fn t() -> LispExp<T> {
        LispExp::Symbol(t_name().clone())
    }

    /// Convert a Rust boolean to `t` or `nil`.
    pub fn boolean(value: bool) -> LispExp<T> {
        if value { Self::t() } else { Self::nil() }
    }

    /// Return whether a fiber has finished, or `None` for another value type.
    ///
    /// A poisoned state lock is treated as finished.
    pub fn fiber_is_done(&self) -> Option<bool> {
        match self {
            LispExp::Fiber(fiber) => Some(
                fiber
                    .0
                    .read()
                    .map(|state| state.status == crate::FiberStatus::Done)
                    .unwrap_or(true),
            ),
            _ => None,
        }
    }

    /// Whether this value is the symbol `nil` or an empty syntax form.
    pub fn is_nil(&self) -> bool {
        match self {
            LispExp::Symbol(s) => s.as_str() == "nil",
            LispExp::Form(l) => l.is_empty(),
            _ => false,
        }
    }

    /// Whether this value is true in Lisp: false only for [`Self::is_nil`].
    /// Zero, empty strings, and empty vectors and maps are true.
    pub fn is_truthy(&self) -> bool {
        !self.is_nil()
    }

    /// Create an immutable Lisp string.
    pub fn string(value: String) -> LispExp<T> {
        LispExp::String(Arc::new(value))
    }

    /// Create a Lisp number without restricting finite or integral values.
    pub fn number(value: f64) -> LispExp<T> {
        LispExp::Number(value)
    }

    /// Prepend a cell without copying `cdr`; a non-list tail creates a dotted pair.
    pub fn cons(car: LispExp<T>, cdr: LispExp<T>) -> LispExp<T> {
        LispExp::Cons(Arc::new(ConsCell { car, cdr }))
    }

    /// Build a proper data list in input order, terminated by nil.
    /// An empty input returns nil.
    pub fn proper_list(items: Vec<LispExp<T>>) -> LispExp<T> {
        items
            .into_iter()
            .rev()
            .fold(LispExp::nil(), |cdr, car| LispExp::cons(car, cdr))
    }

    /// Build a data list in input order ending in `tail`.
    /// An empty input returns `tail` unchanged.
    pub fn improper_list(items: Vec<LispExp<T>>, tail: LispExp<T>) -> LispExp<T> {
        items
            .into_iter()
            .rev()
            .fold(tail, |cdr, car| LispExp::cons(car, cdr))
    }

    /// Iterate cloned heads of the cons prefix, ignoring a dotted tail.
    /// Non-cons values produce an empty iterator. Use [`Self::split_list`]
    /// when the tail must be checked.
    pub fn iter(&self) -> ConsIter<T> {
        ConsIter {
            cursor: self.clone(),
        }
    }

    /// Collect a cons prefix and return its final tail.
    /// A proper list ends in nil. A non-cons value returns `(vec![], self.clone())`.
    pub fn split_list(&self) -> (Vec<LispExp<T>>, LispExp<T>) {
        let mut items = Vec::new();
        let mut cursor = self.clone();
        while let LispExp::Cons(cell) = cursor {
            items.push(cell.car.clone());
            cursor = cell.cdr.clone();
        }
        (items, cursor)
    }

    /// Build executable syntax. Use [`Self::proper_list`] for runtime list data.
    pub fn form(value: Vec<LispExp<T>>) -> LispExp<T> {
        LispExp::Form(Arc::new(LispVec(value)))
    }

    /// Create a vector value, distinct from a data list or syntax form.
    pub fn vec(value: Vec<LispExp<T>>) -> LispExp<T> {
        LispExp::Vector(Arc::new(LispVec(value)))
    }

    /// Create a map with string keys.
    pub fn map(value: HashMap<String, LispExp<T>>) -> LispExp<T> {
        LispExp::Map(Arc::new(LispMap(value)))
    }

    /// Wrap a closure and share its allocation across clones.
    pub fn lambda(value: Lambda<T>) -> LispExp<T> {
        LispExp::Lambda(Arc::new(value))
    }

    /// Wrap a continuation in shared fiber state.
    pub fn fiber(value: FiberState<T>) -> LispExp<T> {
        LispExp::Fiber(SharedFiber(Arc::new(RwLock::new(value))))
    }

    /// Create a native primitive with an optional Lisp docstring.
    ///
    /// Use [`std::borrow::Cow::Borrowed`] for static text or
    /// [`std::borrow::Cow::Owned`] for runtime documentation. Clones share the docstring.
    pub fn primitive(pointer: LispPrimitive<T>, doc: Option<Cow<'static, str>>) -> LispExp<T> {
        LispExp::Primitive {
            pointer,
            doc: Arc::new(doc.unwrap_or(Cow::Borrowed("No documentation provided."))),
        }
    }
}
