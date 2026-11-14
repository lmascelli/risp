//! Readable printing: text that [`Parser::for_data`] reads back into an equal
//! value. Unlike the `Debug` notation, it refuses what cannot be read back.
//!
//! [`Parser::for_data`]: crate::Parser::for_data
use crate::parser::{MAX_READ_DEPTH, reads_as_symbol};
use crate::{LispContext, LispExp};

/// Work left for [`write_readable`]; values carry their nesting depth.
enum Task<'a, T: LispContext> {
    Value(&'a LispExp<T>, usize),
    Text(&'static str),
    Key(&'a str),
}

/// Why a value has no readable text.
#[derive(Clone, Debug, PartialEq)]
pub enum Unreadable<T: LispContext> {
    /// This part has no read syntax: a closure, primitive, native function,
    /// atom, fiber or thread handle; a non-finite number; or a symbol whose
    /// name the reader would split or read as something else.
    Value(LispExp<T>),
    /// Containers are nested deeper than the reader's limit of 256 levels.
    TooDeep,
}

/// Write `exp` so that [`Parser::for_data`] reads it back as an equal value.
///
/// Lists are written as data, dotted tails included; a syntax form is
/// written as the list it spells. Map keys come out in sorted order, bare
/// when they read back as the same symbol and as string literals otherwise.
///
/// # Errors
///
/// Returns [`Unreadable`] naming the first part that cannot be read back.
///
/// [`Parser::for_data`]: crate::Parser::for_data
pub fn write_readable<T: LispContext>(exp: &LispExp<T>) -> Result<String, Unreadable<T>> {
    let mut out = String::new();
    let mut pending = vec![Task::Value(exp, 0)];

    while let Some(task) = pending.pop() {
        let (value, depth) = match task {
            Task::Text(text) => {
                out.push_str(text);
                continue;
            }
            Task::Key(key) if reads_as_symbol(key) => {
                out.push_str(key);
                continue;
            }
            Task::Key(key) => {
                write_string(&mut out, key);
                continue;
            }
            Task::Value(value, depth) => (value, depth),
        };

        // `depth` counts the containers around `value`; the reader refuses
        // to open one more once 256 are open.
        let opens_container = matches!(
            value,
            LispExp::Cons(_) | LispExp::Vector(_) | LispExp::Map(_)
        ) || matches!(value, LispExp::Form(items) if !items.is_empty());
        if opens_container && depth >= MAX_READ_DEPTH {
            return Err(Unreadable::TooDeep);
        }

        match value {
            LispExp::Number(n) if n.is_finite() => {
                // Display never uses an exponent, which the reader lacks, and
                // prints the shortest digits that parse back to the same f64.
                out.push_str(&n.to_string());
            }
            LispExp::String(text) => write_string(&mut out, text),
            LispExp::Symbol(name) if reads_as_symbol(name) => out.push_str(name),
            other if other.is_nil() => out.push_str("nil"),

            LispExp::Cons(_) | LispExp::Form(_) => {
                let mut items = Vec::new();
                let mut tail = None;
                let mut rest = value;
                loop {
                    match rest {
                        LispExp::Cons(cell) => {
                            items.push(&cell.car);
                            rest = &cell.cdr;
                        }
                        LispExp::Form(form) => {
                            items.extend(form.iter());
                            break;
                        }
                        end if end.is_nil() => break,
                        end => {
                            tail = Some(end);
                            break;
                        }
                    }
                }

                out.push('(');
                pending.push(Task::Text(")"));
                if let Some(tail) = tail {
                    pending.push(Task::Value(tail, depth + 1));
                    pending.push(Task::Text(" . "));
                }
                push_values(&mut pending, items, depth + 1);
            }

            LispExp::Vector(items) => {
                out.push('[');
                pending.push(Task::Text("]"));
                push_values(&mut pending, items.iter().collect(), depth + 1);
            }

            LispExp::Map(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_unstable_by(|a, b| a.0.cmp(b.0));

                out.push('{');
                pending.push(Task::Text("}"));
                for (index, (key, item)) in entries.into_iter().enumerate().rev() {
                    pending.push(Task::Value(item, depth + 1));
                    pending.push(Task::Text(" "));
                    pending.push(Task::Key(key));
                    if index > 0 {
                        pending.push(Task::Text(" "));
                    }
                }
            }

            other => return Err(Unreadable::Value(other.clone())),
        }
    }

    Ok(out)
}

/// Queue `items` to be written in order, separated by single spaces.
fn push_values<'a, T: LispContext>(
    pending: &mut Vec<Task<'a, T>>,
    items: Vec<&'a LispExp<T>>,
    depth: usize,
) {
    for (index, item) in items.into_iter().enumerate().rev() {
        pending.push(Task::Value(item, depth));
        if index > 0 {
            pending.push(Task::Text(" "));
        }
    }
}

/// Write a string literal using only escapes the reader decodes.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{1b}' => out.push_str("\\e"),
            '\0' => out.push_str("\\0"),
            other => out.push(other),
        }
    }
    out.push('"');
}
