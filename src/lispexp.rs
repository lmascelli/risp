//! Lisp values, syntax forms, symbol interning, equality, and printing.

use super::{
    ConsCell, ConsIter, FiberState, Lambda, LispContext, LispPrimitive, SharedAtom, SharedFiber,
    SharedThreadHandle,
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
    Form(Arc<Vec<LispExp<T>>>),

    /// Data cons cell; nil terminates a proper list, another value a dotted list.
    Cons(Arc<ConsCell<T>>),

    /// A vector of Lisp values.
    Vector(Arc<Vec<LispExp<T>>>),
    /// A map keyed by strings or symbol names.
    Map(Arc<HashMap<String, LispExp<T>>>),
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

/// Compare cons spines iteratively to avoid stack growth on long flat lists.
/// Closures compare by identity rather than traversing captured environments.
#[allow(unpredictable_function_pointer_comparisons)]
impl<T: LispContext> PartialEq for LispExp<T> {
    fn eq(&self, other: &Self) -> bool {
        use LispExp::*;
        match (self, other) {
            (Cons(_), Cons(_)) => {
                let mut left = self.clone();
                let mut right = other.clone();
                loop {
                    match (&left, &right) {
                        (Cons(a), Cons(b)) => {
                            if Arc::ptr_eq(a, b) {
                                return true;
                            }
                            if a.car != b.car {
                                return false;
                            }
                            let (next_left, next_right) = (a.cdr.clone(), b.cdr.clone());
                            left = next_left;
                            right = next_right;
                        }
                        _ => return left == right,
                    }
                }
            }

            (Lambda(a), Lambda(b)) => Arc::ptr_eq(a, b),

            (Form(a), Form(b)) => a == b,
            (Vector(a), Vector(b)) => a == b,
            (Map(a), Map(b)) => a == b,
            (Number(a), Number(b)) => a == b,
            (Symbol(a), Symbol(b)) => a == b,
            (String(a), String(b)) => a == b,
            (Atom(a), Atom(b)) => a == b,
            (Fiber(a), Fiber(b)) => a == b,
            (Primitive { pointer: a, .. }, Primitive { pointer: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// Format values in Lisp notation. Runtime-only values use opaque `#<...>` forms.
impl<T: LispContext> std::fmt::Debug for LispExp<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LispExp::Form(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", item)?;
                }
                write!(f, ")")
            }

            LispExp::Cons(_) => {
                let (items, tail) = self.split_list();
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", item)?;
                }
                if !tail.is_nil() {
                    write!(f, " . {:?}", tail)?;
                }
                write!(f, ")")
            }

            LispExp::Vector(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", item)?;
                }
                write!(f, "]")
            }

            LispExp::Map(map) => {
                write!(f, "{{")?;
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{} {:?}", key, value)?;
                }
                write!(f, "}}")
            }

            // Restrict the integer cast to a conservative range; format larger values directly.
            LispExp::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{}", n)
                }
            }

            LispExp::Symbol(s) => write!(f, "{}", s),

            LispExp::String(s) => write!(f, "{:?}", s.as_str()),

            LispExp::Lambda(lambda) => {
                write!(f, "#<lambda (")?;
                let mut first = true;
                for param in &lambda.params {
                    if !first {
                        write!(f, " ")?;
                    }
                    first = false;
                    write!(f, "{}", param)?;
                }
                if !lambda.optionals.is_empty() {
                    if !first {
                        write!(f, " ")?;
                    }
                    first = false;
                    write!(f, "&optional")?;
                    for param in &lambda.optionals {
                        write!(f, " {}", param)?;
                    }
                }
                if let Some(rest) = &lambda.rest {
                    if !first {
                        write!(f, " ")?;
                    }
                    write!(f, "&rest {}", rest)?;
                }
                write!(f, ")>")
            }

            LispExp::Primitive { .. } => write!(f, "#<primitive>"),

            // Do not lock or traverse atoms: they may contain themselves,
            // or a writer may be formatting its own locked value.
            LispExp::Atom(_) => write!(f, "#<atom>"),

            LispExp::Fiber(fiber) => match fiber.0.try_read() {
                Ok(state) if state.is_done => write!(f, "#<fiber done>"),
                Ok(_) => write!(f, "#<fiber>"),
                Err(_) => write!(f, "#<fiber running>"),
            },

            LispExp::ThreadHandle(t) => write!(f, "#t<thread-handle>"),
        }
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
            LispExp::Fiber(fiber) => {
                Some(fiber.0.read().map(|state| state.is_done).unwrap_or(true))
            }
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
        LispExp::Form(Arc::new(value))
    }

    /// Create a vector value, distinct from a data list or syntax form.
    pub fn vec(value: Vec<LispExp<T>>) -> LispExp<T> {
        LispExp::Vector(Arc::new(value))
    }

    /// Create a map with string keys.
    pub fn map(value: HashMap<String, LispExp<T>>) -> LispExp<T> {
        LispExp::Map(Arc::new(value))
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
