use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RuntimeId(u64);


pub struct Runtime {
    id: RuntimeId,
}

static NEXT_RUNTIME_ID: AtomicU64 = AtomicU64::new(1);

impl Runtime {
    pub fn new() -> Self {
        Self {
            id: loop {
                let current = NEXT_RUNTIME_ID.load(Ordering::Relaxed);
                let next = match current.checked_add(1) {
                    Some(value) => value,
                    None => panic!("Runtime identifiers exhausted"),
                };
                // Check if some other thread has already changed the
                // NEXT_RUNTIME_ID
                let result = NEXT_RUNTIME_ID.compare_exchange(
                    current,
                    next,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
                match result {
                    Ok(previous) => break RuntimeId(previous),
                    Err(_) => continue,
                }
            }
        }
    }
}