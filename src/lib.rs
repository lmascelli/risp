//! An embeddable Lisp interpreter with lexical closures, native primitives,
//! cooperative fibers, and optional execution metering.
//!
//! Implement [`LispContext`] for host services, create an [`Env`], install the
//! standard functions with [`setup_base_env`], then read forms with [`Parser`]
//! and evaluate them with [`eval`]. Variables, functions, and macros have
//! separate namespaces. Syntax forms and runtime data lists are distinct;
//! use [`LispExp::proper_list`] when returning lists from native functions.
//!
//! # Example
//!
//! ```
//! use risp::{Env, LispContext, LispExp, Parser, eval, setup_base_env};
//!
//! #[derive(Clone, Debug, PartialEq)]
//! struct Host;
//! impl LispContext for Host {}
//!
//! let env = Env::new_root();
//! setup_base_env(env.clone());
//! let form = Parser::new("(+ 20 22)").next::<Host>().unwrap();
//! assert_eq!(eval(&form, env, &Host).unwrap(), LispExp::number(42.0));
//! ```
//!
//! Register native functions with [`Env::set_function`] and [`LispExp::primitive`];
//! [`LispPrimitive`] includes a complete example. Use [`call_callable`] to invoke
//! callbacks from Rust with already evaluated arguments. [`bootstrap_vm`] also
//! runs a diagnostic script and leaves its bindings in the returned environment;
//! the example above initializes an environment without that script.
//!
//! The default context does not meter execution. To bound charged work, connect
//! [`FuelMeter::consume`] to [`LispContext::consume_fuel`], keep a
//! [`FuelMeter::begin`] guard alive during evaluation, and arm spawned threads
//! in [`LispContext::begin_thread_evaluation`]. Native functions must charge
//! their own potentially expensive work. Fuel does not impose a wall-clock timeout.
//!
mod context;
mod environment;
mod error;
mod eval;
mod fuel;
mod lispexp;
mod parser;
mod source_map;
mod types;
mod utils;

pub use context::LispContext;
pub use environment::{Env, VARIABLE_DOCUMENTATION};
pub use error::EvalError;
pub use eval::{eval, resume_frames};
pub use fuel::measure;
pub use fuel::{DEFAULT_FUEL, FuelMeter, FuelScope, set_remaining};
pub use lispexp::LispExp;
pub use parser::{Parser, form_to_data};
pub use source_map::{Location, SourceMap};
use types::{
    ConsCell, ConsIter, FiberState, LispMap, LispVec, SharedAtom, SharedFiber, SharedThreadHandle,
};
pub use types::{Frame, Lambda, LispPrimitive};
use utils::{
    bind_lambda_args, condition_matches, data_to_form, error_data, error_symbol, is_container,
    parse_lambda_params, take_container,
};
pub use utils::{exact_arity, some_arguments};

mod base;
mod handshake;
pub use base::{call_callable, lisp_display, setup_base_env};
pub use handshake::bootstrap_vm;

#[cfg(test)]
use parser::Token;
mod tests;
