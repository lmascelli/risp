//! Cooperative fibers, resumed synchronously by the calling thread.
use super::*;

const RESUME_DOC: &str = "(resume FIBER): Carry FIBER on from wherever it stopped, and return \
                 the value it stopped with. Returns nil once FIBER has nothing left to run.\n\n\
                 If FIBER is already running, returns nil without changing its state.\n\n\
                 A fiber runs until it reaches a `yield\' or until its body is finished, \
                 whichever comes first. A `yield\' stops it *inside* a form, and the next \
                 `resume\' picks up at that exact spot with its variables as it left them.\n\n\
                 The body is a `progn\': `(fiber A B C)\' runs all three in one `resume\' unless \
                 one of them yields. Put a `yield\' between them to stop after each.\n\n\
                 An error inside a fiber ends it: the position it stopped at is gone, so there is \
                 nothing left to come back to, and `resume\' would otherwise silently restart it \
                 somewhere it had already been.\n\n\
                 Example:\n\
                 (setq f (fiber (log \"a\") (yield 1) (log \"b\") 2))\n\
                 (resume f) ; logs \"a\", => 1\n\
                 (resume f) ; logs \"b\", => 2\n\
                 (resume f) => nil ; fiber is done";

/// Retire a claimed continuation if evaluation unwinds through a Rust panic
/// or exits before its normal state transition has been published.
struct RunningFiber<'a, T: LispContext> {
    fiber: &'a crate::SharedFiber<T>,
    finished: bool,
}

impl<T: LispContext> Drop for RunningFiber<'_, T> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let pending = {
            let mut state = self
                .fiber
                .0
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.status = crate::FiberStatus::Done;
            std::mem::take(&mut state.pending)
        };
        // Release values after releasing the state lock.
        drop(pending);
    }
}

fn primitive_resume<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    let Some(LispExp::Fiber(shared_fiber)) = args.first() else {
        return match args.first() {
            None => Err(EvalError::WrongNumberOfArguments {
                expected: 1,
                got: 0,
            }),
            Some(other) => Err(EvalError::WrongArgumentType {
                expected: "Fiber".into(),
                got: other.clone(),
            }),
        };
    };

    let frames = {
        let mut fiber = shared_fiber
            .0
            .write()
            .map_err(|_| EvalError::UncorrectFunctionDefinition)?;
        match fiber.status {
            crate::FiberStatus::Done | crate::FiberStatus::Running => {
                return Ok(LispExp::nil());
            }
            crate::FiberStatus::Suspended => {
                fiber.status = crate::FiberStatus::Running;
                std::mem::take(&mut fiber.pending)
            }
        }
    };

    // Keep no state lock while running Lisp: reentry may inspect this fiber.
    let mut running = RunningFiber {
        fiber: shared_fiber,
        finished: false,
    };
    let outcome = resume_frames(frames, ctx);

    let mut fiber = shared_fiber
        .0
        .write()
        .map_err(|_| EvalError::UncorrectFunctionDefinition)?;
    let result = match outcome {
        Err(EvalError::Yielded { value, frames }) => {
            fiber.pending = frames;
            fiber.status = crate::FiberStatus::Suspended;
            Ok(value)
        }
        Ok(value) => {
            fiber.status = crate::FiberStatus::Done;
            Ok(value)
        }
        Err(other) => {
            fiber.status = crate::FiberStatus::Done;
            Err(other)
        }
    };
    running.finished = true;
    result
}

const FIBER_DONE_P_DOC: &str = "(fiber-done-p FIBER): t when FIBER has nothing left to run, nil \
                 while it still has. Signals if FIBER is not a fiber.\n\n\
                 What a scheduler asks so it can stop resuming one. A fiber that is merely \
                 *suspended* is not done -- it is in the middle of a form, waiting to be resumed \
                 -- so this is the only honest way to tell a finished task from a parked one; \
                 `resume\' answering nil cannot, since a fiber may perfectly well yield nil.\n\n\
                 Example:\n\
                 (while (not (fiber-done-p task)) (resume task))";

fn primitive_fiber_done_p<T: LispContext>(
    args: &[LispExp<T>],
    _env: Arc<Env<T>>,
    _ctx: &T,
) -> Result<LispExp<T>, EvalError<T>> {
    let Some(exp) = args.first() else {
        return Err(EvalError::WrongNumberOfArguments {
            expected: 1,
            got: 0,
        });
    };
    match exp.fiber_is_done() {
        Some(done) => Ok(LispExp::boolean(done)),
        None => Err(EvalError::WrongArgumentType {
            expected: "Fiber".into(),
            got: exp.clone(),
        }),
    }
}

/// Register this module's primitives and Lisp docstrings.
pub(super) fn install<T: LispContext>(into: &Registry<T>) {
    into.function("fiber-done-p", primitive_fiber_done_p, FIBER_DONE_P_DOC);
    into.function("resume", primitive_resume, RESUME_DOC);
}
