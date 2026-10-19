//! Hooks for embedding the interpreter in a host application.
use super::{EvalError, Location, SourceMap};
use std::sync::Arc;

/// Host services used during evaluation.
///
/// All hooks have defaults: no fuel limit, diagnostics, or backtrace recording.
/// Clones should share any host state that spawned evaluations need to access.
pub trait LispContext: Clone + PartialEq + std::fmt::Debug + Send + Sync + 'static {
    /// Charge `amount` execution units against the host's budget.
    ///
    /// Return [`EvalError::OutOfFuel`] when the charge cannot be paid.
    /// The default accepts every charge without metering.
    fn consume_fuel(&self, _amount: u32) -> Result<(), EvalError<Self>> {
        Ok(())
    }

    /// Receive a diagnostic message from the interpreter. The default discards it.
    fn log_diagnostic(&self, _msg: &str) {}

    /// Run the cleanup forms of `unwind-protect`.
    ///
    /// Hosts may provide a bounded reserve after ordinary fuel is exhausted.
    /// Reserve fuel must not escape into ordinary evaluation; nested cleanups
    /// must share the remaining reserve rather than refill it.
    fn with_unwind<R>(&self, cleanup: impl FnOnce() -> R) -> R {
        cleanup()
    }

    /// Initialize host state on a newly spawned evaluation thread.
    ///
    /// Override this to arm thread-local fuel using [`crate::FuelMeter::arm_thread`].
    /// No matching end call is made. The default does nothing.
    fn begin_thread_evaluation(&self) {}

    /// Record a direct evaluator call, labeled with its name or `<lambda>`.
    ///
    /// Successful and suspended calls pop their frame; failed calls leave it for
    /// a backtrace. Hosts implementing this hook should also implement the other
    /// frame hooks and clear retained frames after reporting an uncaught error.
    ///
    /// Tail calls replace their caller, so the stack records active Rust call
    /// nesting rather than the complete Lisp call history. The default does nothing.
    fn push_call_frame(&self, _frame: &str) {}

    /// Remove the most recent call frame. The default does nothing.
    fn pop_call_frame(&self) {}

    /// Return the current frame depth, including any error-origin entry.
    ///
    /// Code that handles an evaluation error uses this with
    /// [`Self::truncate_call_frames`] to discard the handled failure's frames.
    /// The default returns zero.
    fn call_frame_depth(&self) -> usize {
        0
    }

    /// Discard entries above `depth`; leave a shallower stack unchanged.
    /// The default does nothing.
    fn truncate_call_frames(&self, _depth: usize) {}

    /// Return the source map used for error locations, or `None` to disable them.
    ///
    /// Use the same map when constructing [`crate::Parser::with_source_map`].
    /// The evaluator consults it only after an evaluation fails.
    fn source_map(&self) -> Option<Arc<SourceMap>> {
        None
    }

    /// Record an error location, reported innermost first for located forms.
    ///
    /// Keep the first origin as a separate stack entry, counted by
    /// [`Self::call_frame_depth`] and removed by [`Self::truncate_call_frames`].
    /// Ignore enclosing origins while that entry is innermost. This lets
    /// `condition-case` remove handled origins while `unwind-protect` preserves
    /// the original failure during cleanup. The default does nothing.
    fn note_error_origin(&self, _at: &Location) {}

    /// Reserved hook for attaching a failed call's location to a backtrace frame.
    ///
    /// The current evaluator does not invoke this hook. Error origins are
    /// reported through [`Self::note_error_origin`]. The default does nothing.
    fn note_call_site(&self, _at: &Location) {}
}
