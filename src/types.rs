//! Closure, continuation, and shared runtime types.

use super::{Env, EvalError, LispContext, LispExp};
use std::sync::{Arc, RwLock};

/// A runtime list cell whose tail may be another cell or any Lisp value.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsCell<T: LispContext> {
    /// Value at the head of the cell.
    pub car: LispExp<T>,
    /// Remaining list or dotted tail.
    pub cdr: LispExp<T>,
}

/// Unlink uniquely owned cons tails iteratively to avoid recursive stack overflow.
/// Stop at a shared cell: its remaining owners still need the tail.
impl<T: LispContext> Drop for ConsCell<T> {
    fn drop(&mut self) {
        let mut next = std::mem::replace(&mut self.cdr, LispExp::nil());
        while let LispExp::Cons(cell) = next {
            match Arc::try_unwrap(cell) {
                Ok(mut owned) => next = std::mem::replace(&mut owned.cdr, LispExp::nil()),
                Err(_) => break,
            }
        }
    }
}

/// Iterator over the proper prefix of a cons chain, excluding a dotted tail.
pub struct ConsIter<T: LispContext> {
    pub(super) cursor: LispExp<T>,
}

impl<T: LispContext> Iterator for ConsIter<T> {
    type Item = LispExp<T>;
    fn next(&mut self) -> Option<Self::Item> {
        match &self.cursor {
            LispExp::Cons(cell) => {
                let cell = cell.clone();
                self.cursor = cell.cdr.clone();
                Some(cell.car.clone())
            }
            _ => None,
        }
    }
}

/// A lexical closure with required, optional, and rest parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Lambda<T: LispContext> {
    /// Required parameter names, in argument order.
    pub params: Vec<String>,
    /// Optional parameter names; omitted arguments bind to nil.
    pub optionals: Vec<String>,
    /// Parameter collecting arguments beyond the required and optional ones.
    /// Without it, extra arguments cause an arity error.
    pub rest: Option<String>,
    /// Body expressions, evaluated in order.
    pub body: Vec<LispExp<T>>,
    /// Captured lexical environment used as the parent of each call scope.
    pub env: Arc<Env<T>>,
    /// Optional Lisp docstring.
    pub doc: Option<Arc<String>>,
}

/// A mutable Lisp value shared through a read/write lock. Equality uses allocation identity.
#[derive(Clone, Debug)]
pub struct SharedAtom<T: LispContext>(pub Arc<RwLock<LispExp<T>>>);

impl<T: LispContext> PartialEq for SharedAtom<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// A suspended body or loop, resumed from the innermost frame outward.
///
/// Only statement positions can record continuations. Yields in arguments,
/// conditions, or native primitive execution return [`EvalError::YieldNotAllowed`].
#[derive(Debug)]
pub enum Frame<T: LispContext> {
    /// Resume a body in `env` at `from`, retaining its final value.
    Body {
        /// Expressions of the suspended block.
        forms: Vec<LispExp<T>>,
        /// Index of the next expression to evaluate.
        from: usize,
        /// Lexical environment retained by the continuation.
        env: Arc<Env<T>>,
    },
    /// Finish an interrupted `while` iteration, then retest the condition.
    While {
        /// Loop condition to retest after the interrupted iteration.
        condition: LispExp<T>,
        /// Expressions of the suspended block.
        forms: Vec<LispExp<T>>,
        /// Index of the next expression to evaluate.
        from: usize,
        /// Lexical environment retained by the continuation.
        env: Arc<Env<T>>,
    },
}

/// Frames compare by identity because they represent positions in a running program.
impl<T: LispContext> PartialEq for Frame<T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// A fiber's remaining continuation and completion state.
#[derive(Debug)]
pub struct FiberState<T: LispContext> {
    /// Innermost frame first. An empty stack can also mean the fiber is
    /// currently running; consult `is_done` for completion.
    pub pending: Vec<Frame<T>>,
    /// Whether the fiber has completed or terminated after an error.
    pub is_done: bool,
}

/// Shared fiber state protected by a read/write lock.
#[derive(Clone, Debug)]
pub struct SharedFiber<T: LispContext>(pub Arc<RwLock<FiberState<T>>>);

impl<T: LispContext> PartialEq for SharedFiber<T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// Native function signature: evaluated arguments, call environment, and host context.
///
/// The environment is the caller's scope; a lambda instead uses its captured
/// environment. Arguments are values, not syntax. Charge host fuel for expensive
/// work and use [`LispExp::proper_list`] for runtime lists.
///
/// Return a value or an [`EvalError`]. This signature accepts a function pointer;
/// register stateful host services through the context rather than a capturing closure.
///
/// # Example
///
/// ```
/// use risp::{Env, EvalError, LispContext, LispExp, exact_arity};
/// use std::sync::Arc;
///
/// #[derive(Clone, Debug, PartialEq)]
/// struct Host;
/// impl LispContext for Host {}
///
/// fn double(
///     args: &[LispExp<Host>],
///     _env: Arc<Env<Host>>,
///     _ctx: &Host,
/// ) -> Result<LispExp<Host>, EvalError<Host>> {
///     exact_arity(args, 1)?;
///     match &args[0] {
///         LispExp::Number(n) => Ok(LispExp::number(n * 2.0)),
///         value => Err(EvalError::WrongArgumentType {
///             expected: "Number".into(),
///             got: value.clone(),
///         }),
///     }
/// }
///
/// let env = Env::new_root();
/// env.set_function(
///     "host-double".into(),
///     LispExp::primitive(double, Some("(host-double NUMBER): Double NUMBER.".into())),
/// );
/// let value = risp::call_callable(
///     &LispExp::symbol("host-double".into()),
///     &[LispExp::number(21.0)],
///     env,
///     &Host,
/// ).unwrap();
/// assert_eq!(value, LispExp::number(42.0));
/// ```
///
pub type LispPrimitive<T> = fn(&[LispExp<T>], Arc<Env<T>>, &T) -> Result<LispExp<T>, EvalError<T>>;
