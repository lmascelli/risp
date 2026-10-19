//! Thread-local execution budgets and bounded cleanup reserves.
//!
//! Hosts connect [`FuelMeter::consume`] to [`crate::LispContext::consume_fuel`]
//! and define evaluation scopes with [`FuelMeter::begin`].

use std::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

/// Default fuel allowance: 10,000,000 execution units.
///
/// Fuel bounds charged work, not elapsed time; native primitives must charge
/// for work beyond ordinary evaluation steps.
pub const DEFAULT_FUEL: u32 = 10_000_000;

thread_local! {
    /// Remaining fuel is thread-local so concurrent evaluations cannot spend
    /// or refill each other's allowance. Threads without a scope start bounded
    /// by `DEFAULT_FUEL`.
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

/// Guard that closes a metered scope when dropped.
///
/// Keep it alive for the evaluation and drop it on the thread that created it.
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

/// The budget was exhausted. Map this result to [`crate::EvalError::OutOfFuel`]
/// when implementing [`crate::LispContext::consume_fuel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exhausted;

/// A configurable allowance for thread-local execution scopes.
///
/// Meters configure budgets, but remaining fuel and nesting depth are shared
/// by all meters on the same thread. Nested scopes do not refill, even when
/// opened through a different meter. Counters on different threads are independent.
///
/// # Example
///
/// ```
/// use risp::{Env, EvalError, FuelMeter, LispContext, LispExp, Parser};
/// use std::sync::Arc;
///
/// #[derive(Clone, Debug)]
/// struct Host(Arc<FuelMeter>);
///
/// impl PartialEq for Host {
///     fn eq(&self, other: &Self) -> bool {
///         Arc::ptr_eq(&self.0, &other.0)
///     }
/// }
///
/// impl LispContext for Host {
///     fn consume_fuel(&self, amount: u32) -> Result<(), EvalError<Self>> {
///         self.0.consume(amount).map_err(|_| EvalError::OutOfFuel)
///     }
///
///     fn begin_thread_evaluation(&self) {
///         self.0.arm_thread();
///     }
///
///     fn with_unwind<R>(&self, cleanup: impl FnOnce() -> R) -> R {
///         self.0.with_unwind(100, cleanup)
///     }
/// }
///
/// let host = Host(Arc::new(FuelMeter::new(1_000)));
/// let env = Env::new_root();
/// risp::setup_base_env(env.clone());
/// let form = Parser::new("(+ 20 22)").next::<Host>().unwrap();
/// let _scope = host.0.begin();
/// assert_eq!(risp::eval(&form, env, &host).unwrap(), LispExp::number(42.0));
/// ```
///
#[derive(Debug)]
pub struct FuelMeter {
    budget: AtomicU32,
}

impl FuelMeter {
    /// Configure an allowance for future outermost scopes; does not refill this thread.
    pub fn new(budget: u32) -> Self {
        Self {
            budget: AtomicU32::new(budget),
        }
    }

    /// Charge this thread's active allowance.
    ///
    /// Returns an exhaustion error when insufficient fuel remains. A failed charge
    /// does not subtract fuel; cleanup may use its configured reserve.
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

    /// Reset this thread to the configured budget without opening a scope.
    ///
    /// Use on a newly spawned thread; this also resets its cleanup reserve.
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

    /// Set the budget for future scopes and replace this thread's remaining fuel.
    ///
    /// The new value can raise or lower the current allowance.
    pub fn set_budget(&self, budget: u32) {
        self.budget.store(budget, Ordering::Relaxed);
        FUEL.set(budget);
    }

    /// Run cleanup code on ordinary fuel, falling back to a bounded reserve.
    ///
    /// Unspent ordinary fuel is preserved. Reserve fuel cannot escape.
    /// The allowance is shared until the next outermost `begin()` or
    /// `arm_thread()`. Nested cleanups cannot refill it.
    pub fn with_unwind<R>(&self, allowance: u32, cleanup: impl FnOnce() -> R) -> R {
        if UNWINDING.get() {
            return cleanup();
        }

        let reserve = UNWIND_FUEL.get().unwrap_or(allowance).min(allowance);

        UNWIND_FUEL.set(Some(reserve));
        UNWINDING.set(true);

        DEPTH.with(|depth| depth.set(depth.get() + 1));

        let _restore = UnwindBudget {
            reserve_start: reserve,
        };

        cleanup()
    }
}

/// Replace the calling thread's remaining fuel with `amount`.
///
/// This can raise or lower the allowance without changing any meter's budget.
pub fn set_remaining(amount: u32) {
    FUEL.set(amount);
}

/// Restore fuel counters when measurement ends, including during unwinding.
struct RestoreFuel(u32, Option<u32>, Option<u32>, u64);

impl Drop for RestoreFuel {
    fn drop(&mut self) {
        FUEL.set(self.0);
        UNWIND_FUEL.set(self.1);
        ORDINARY_FUEL.set(self.2);
        UNWIND_SPENT.set(self.3);
    }
}

/// Run `body` with `u32::MAX` fuel and return `(result, units_spent)`.
///
/// Includes cleanup-reserve charges and restores the previous counters when
/// the call ends, including during panic unwinding. The configured budget
/// is unchanged; the measurement allowance is large but finite.
pub fn measure<T, F: FnOnce() -> T>(meter: &FuelMeter, body: F) -> (T, u64) {
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
        .saturating_add(UNWIND_SPENT.get().saturating_sub(unwind_before));

    drop(scope);
    drop(restore);

    (value, spent)
}
