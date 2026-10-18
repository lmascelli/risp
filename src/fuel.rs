//! ========================================================================== //
//!               +------------------------------------------+
//!               |  Execution metering for the interpreter. |
//!               +------------------------------------------+
//! The evaluator charges the host on every step through
//! `LispContext::consume_fuel`, but how large a budget is, and when it
//! refills, is host *policy*. This module supplies the *mechanism* that policy
//! needs, so that each embedder does not have to re-derive the thread-local
//! bookkeeping, the nesting rules, and the arithmetic edge cases for itself.
//!
//! It knows nothing about context. A host decides what constitutes
//! one metered unit of work by wrapping it in [`FuelMeter::begin`].
//! ========================================================================== //

use std::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

/// Default steps one metered scope may run before it is aborted.
///
/// This is a runaway-loop guard, not a resource quota: it exists so a mistyped
/// `(while t ...)` cannot hang its host forever. Sized from the interpreter
/// performance suite's measured cost per eval step -- roughly 100ns in release
/// and 500ns in debug -- so a runaway scope costs about a second of
/// unresponsiveness in release and a few seconds in debug before control comes
/// back. Ordinary work uses a vanishing fraction of it.
pub const DEFAULT_FUEL: u32 = 10_000_000;

thread_local! {
    /// Steps remaining for the metered scope running on *this* thread.
    ///
    /// Thread-local rather than shared state on the meter, for two reasons.
    /// Correctness: the `(spawn ...)` special form evaluates on a second
    /// thread while sharing the host context, so a shared counter would let
    /// that thread spend the foreground scope's budget -- and let a new
    /// foreground scope refill it underneath a running background one. Speed:
    /// only the owning thread can reach its own copy, so no synchronisation is
    /// required at all, making this a plain `Cell` load/store rather than an
    /// atomic read-modify-write on a line other cores may be contending for.
    ///
    /// The `const` initialiser avoids the hidden "has this thread initialised
    /// it yet?" branch a non-const one would add to every access. It starts at
    /// `DEFAULT_FUEL` rather than zero so that a thread which never opens a
    /// scope is still *bounded* -- failing safe -- instead of either dying on
    /// its first step or running unmetered.
    static FUEL: Cell<u32> = const { Cell::new(DEFAULT_FUEL) };

    /// Nesting depth of [`FuelMeter::begin`] on this thread. Only the 0 -> 1
    /// transition refills, so code that re-enters the evaluator cannot hand
    /// itself a fresh budget partway through an existing scope.
    static DEPTH: Cell<u32> = const { Cell::new(0) };

    // Cleanup reserve shared by one outermost evaluation scope.
    static UNWIND_FUEL: Cell<Option<u32>> = const { Cell::new(None) };

    static UNWINDING: Cell<bool> = const { Cell::new(false) };

    // Saved ordinary fuel while the cleanup uses its reserve.
    static ORDINARY_FUEL: Cell<Option<u32>> = const { Cell::new(None) };

    // Reserve consumption accumulated when a cleanup finishes.
    static UNWIND_SPENT: Cell<u64> = const { Cell::new(0) };
}

/// Guard returned by [`FuelMeter::begin`]; closes the scope when dropped.
///
/// RAII rather than paired begin/end calls so that an early return or a `?`
/// cannot leave the depth stuck above zero, which would permanently prevent
/// any further refill.
///
/// It borrows nothing. The scope's whole state is the thread-local depth, so
/// the guard never touches the meter again after `begin` has read the budget
/// off it -- and a tether to the meter would have meant a host could not keep
/// one behind a lock, which is exactly where a host's state usually lives.
#[must_use = "the metered scope ends as soon as this guard is dropped"]
pub struct FuelScope {
    _private: (),
}

impl Drop for FuelScope {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

struct UnwindBudget {
    reserve_start: u32,
}

impl Drop for UnwindBudget {
    fn drop(&mut self) {
        if let Some(ordinary_fuel) = ORDINARY_FUEL.replace(None) {
            let remaining = FUEL.get().min(self.reserve_start);

            UNWIND_SPENT.set(
                UNWIND_SPENT
                    .get()
                    .saturating_add(u64::from(self.reserve_start - remaining)),
            );

            UNWIND_FUEL.set(Some(remaining));
            FUEL.set(ordinary_fuel);
        }

        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        UNWINDING.set(false);
    }
}

/// A refillable execution budget shared by every thread of one host.
///
/// The configured budget lives here (shared, read once per scope); the
/// remaining count lives in thread-local storage (private, touched every
/// step). That split is deliberate: the hot counter never needs
/// synchronisation, while the setting stays global and adjustable.
/// What [`FuelMeter::consume`] reports when the budget is gone.
///
/// Deliberately not an `EvalError`: the meter is host-agnostic machinery and
/// has no business naming the interpreter's error type, which is generic over
/// the context. Callers map it to `EvalError::OutOfFuel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exhausted;

#[derive(Debug)]
pub struct FuelMeter {
    budget: AtomicU32,
}

impl FuelMeter {
    pub fn new(budget: u32) -> Self {
        Self {
            budget: AtomicU32::new(budget),
        }
    }

    pub fn consume(&self, amount: u32) -> Result<(), Exhausted> {
        FUEL.with(|fuel| match fuel.get().checked_sub(amount) {
            Some(remaining) => {
                fuel.set(remaining);
                Ok(())
            }
            None => {
                if UNWINDING.get() && ORDINARY_FUEL.get().is_none() {
                    let reserve = UNWIND_FUEL.get().unwrap_or(0);

                    if let Some(remaining) = reserve.checked_sub(amount) {
                        ORDINARY_FUEL.set(Some(fuel.get()));
                        fuel.set(remaining);
                        return Ok(());
                    }
                }
                Err(Exhausted)
            }
        })
    }

    /// Open a metered scope, refilling this thread's budget if it is the
    /// outermost one. Nested calls only deepen the count.
    pub fn begin(&self) -> FuelScope {
        DEPTH.with(|depth| {
            if depth.get() == 0 {
                FUEL.set(self.budget.load(Ordering::Relaxed));
                UNWIND_FUEL.set(None);
                UNWIND_SPENT.set(0);
            }
            depth.set(depth.get() + 1);
        });
        FuelScope { _private: () }
    }

    /// Arm the calling thread's budget without opening a scope, for a freshly
    /// spawned thread that has no enclosing scope to nest inside. Without this
    /// such a thread would run on the compile-time `DEFAULT_FUEL` rather than
    /// the host's configured budget.
    pub fn arm_thread(&self) {
        FUEL.set(self.budget.load(Ordering::Relaxed));
        UNWIND_FUEL.set(None);
        UNWIND_SPENT.set(0);
    }

    /// Raise this thread's remaining fuel to at least `amount`, without
    /// opening a scope or changing the configured budget.
    pub fn grant(&self, amount: u32) {
        FUEL.with(|fuel| {
            if fuel.get() < amount {
                fuel.set(amount);
            }
        });
    }

    /// Set the budget future scopes receive, and top the current thread's
    /// remaining fuel up to it -- so code that knows it will be expensive can
    /// raise its own ceiling as its first act rather than having to restart.
    /// This is also useful to embedders when configuring a host runtime.
    pub fn set_budget(&self, budget: u32) {
        self.budget.store(budget, Ordering::Relaxed);
        FUEL.set(budget);
    }

    /// Run cleanup code on ordinary fuel, falling back to a bounded reserve.
    ///
    /// Unspent ordinary fuel is preserved. Reserve fuel cannot escape.
    /// The allowance is shared until the next outermost `begin()` or
    /// `arm_thread()`. Nested cleanups cannot refill it.
    pub fn with_unwind<R>(
        &self,
        allowance: u32,
        cleanup: impl FnOnce() -> R,
    ) -> R {
        if UNWINDING.get() {
            return cleanup();
        }

        let reserve = UNWIND_FUEL
            .get()
            .unwrap_or(allowance)
            .min(allowance);

        UNWIND_FUEL.set(Some(reserve));
        UNWINDING.set(true);

        DEPTH.with(|depth| depth.set(depth.get() + 1));

        let _restore = UnwindBudget {
            reserve_start: reserve,
        };

        cleanup()
    }
}

/// Set the calling thread's remaining fuel to exactly AMOUNT, whether that
/// raises it or lowers it.
///
/// # Why this is not [`FuelMeter::grant`]
///
/// `grant` only ever raises, because it exists for a cleanup that must run
/// after the budget is spent. This is for the opposite case: a host that runs
/// *repeated, short* pieces of work on a thread of its own, and wants each one
/// bounded by its own small allowance rather than by the editor-sized budget
/// the thread would otherwise carry. Raising-only cannot express that -- the
/// first turn would be handed `DEFAULT_FUEL` and a runaway one would hold the
/// thread for a second.
///
/// # Why it is a free function
///
/// It sets a thread-local and reads no meter, so taking `&self` would be a
/// parameter that exists only to look symmetrical -- and it would mean holding
/// whatever lock the meter lives behind to touch state the meter does not own.
pub fn set_remaining(amount: u32) {
    FUEL.set(amount);
}

/// Run `body` with an effectively unlimited budget and report how much fuel it
/// spent, alongside its result.
struct RestoreFuel(u32, Option<u32>, Option<u32>, u64);

impl Drop for RestoreFuel {
    fn drop(&mut self) {
        FUEL.set(self.0);
        UNWIND_FUEL.set(self.1);
        ORDINARY_FUEL.set(self.2);
        UNWIND_SPENT.set(self.3);
    }
}

pub fn measure<T, F: FnOnce() -> T>(
    meter: &FuelMeter,
    body: F,
) -> (T, u64) {
    let restore = RestoreFuel(
        FUEL.get(),
        UNWIND_FUEL.get(),
        ORDINARY_FUEL.get(),
        UNWIND_SPENT.get(),
    );

    let scope = meter.begin();
    let unwind_before = UNWIND_SPENT.get();

    FUEL.set(u32::MAX);

    let value = body();

    let spent = u64::from(u32::MAX - FUEL.get())
        .saturating_add(
            UNWIND_SPENT.get().saturating_sub(unwind_before),
        );

    drop(scope);
    drop(restore);

    (value, spent)
}