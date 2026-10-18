//! Source locations indexed by syntax-form allocation.
//!
//! Each entry keeps a weak reference to prevent address reuse while indexed,
//! without retaining the form itself. Dead entries are swept during recording.
use super::{LispContext, LispExp};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, Weak};

/// Inclusive source span of a parenthesized syntax form.
///
/// Columns count Unicode scalar values, not bytes or display cells. Line numbers
/// start at the value supplied to [`crate::Parser::with_source_map`], normally 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// File path or synthetic source name such as `<eval-string>`.
    pub file: Arc<str>,
    /// Line of the opening parenthesis.
    pub line: u32,
    /// Column of the opening parenthesis, starting at 1.
    pub column: u32,
    /// Line of the closing parenthesis.
    pub end_line: u32,
    /// Column of the closing parenthesis, starting at 1.
    pub end_column: u32,
}

/// Format the start position as `file:line:column`.
impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

#[derive(Default)]
struct Table {
    /// Address of a form's allocation -> the form, weakly, and where it was.
    forms: HashMap<usize, (Weak<dyn Send + Sync>, Location)>,
    /// Entry count that triggers the next sweep of expired forms.
    sweep_at: usize,
}

/// Thread-safe source locations keyed by syntax-form allocation identity.
///
/// Create an empty map with [`Self::default`] and share it between the parser
/// and host context. Cloned forms retain their location; rebuilt forms do not.
#[derive(Default)]
pub struct SourceMap {
    table: Mutex<Table>,
}

fn address<T: LispContext>(form: &Arc<Vec<LispExp<T>>>) -> usize {
    Arc::as_ptr(form) as *const () as usize
}

impl SourceMap {
    /// Associate `form` with `at`, replacing any location for that allocation.
    /// The map retains a weak reference and does not keep the form alive.
    pub fn record<T: LispContext>(&self, form: &Arc<Vec<LispExp<T>>>, at: Location) {
        let weak = Arc::downgrade(form);
        let weak: Weak<dyn Send + Sync> = weak;
        let mut table = self
            .table
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if table.forms.len() >= table.sweep_at {
            table.forms.retain(|_, (weak, _)| weak.strong_count() > 0);
            // Grow geometrically to amortize sweeps over future insertions.
            table.sweep_at = (table.forms.len() * 2).max(1024);
        }
        table.forms.insert(address(form), (weak, at));
    }

    /// Return a recorded location for this exact form allocation, if present.
    pub fn locate<T: LispContext>(&self, form: &Arc<Vec<LispExp<T>>>) -> Option<Location> {
        let table = self
            .table
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (weak, at) = table.forms.get(&address(form))?;
        (weak.strong_count() > 0).then(|| at.clone())
    }
}
