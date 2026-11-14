//! JSON text to Lisp values and back, written by hand.
//!
//! Objects become maps, arrays vectors, strings strings and numbers numbers.
//! `true` is `t`; `false` and `null` are the symbols `false` and `null`, so
//! neither is confused with `nil`, the empty list. Both directions keep an
//! explicit stack, so deep nesting does not consume the Rust stack.
use crate::{LispContext, LispExp};
use std::collections::HashMap;

/// A JSON text that could not be parsed, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    /// What was wrong, in a few words.
    pub message: String,
    /// One-based line of the fault.
    pub line: usize,
    /// One-based column of the fault, counted in characters.
    pub column: usize,
    /// Zero-based offset of the fault from the start, in characters.
    pub position: usize,
}

impl JsonError {
    fn at(text: &str, byte: usize, message: impl Into<String>) -> Self {
        let before = &text[..byte];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        Self {
            message: message.into(),
            line: before.matches('\n').count() + 1,
            column: before[line_start..].chars().count() + 1,
            position: before.chars().count(),
        }
    }
}

/// A container being filled while parsing.
enum Open<T: LispContext> {
    Array(Vec<LispExp<T>>),
    Object(HashMap<String, LispExp<T>>, String),
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn error(&self, message: impl Into<String>) -> JsonError {
        JsonError::at(self.text, self.pos, message)
    }

    /// An error for the byte at the cursor: either the input ended, or the
    /// character found there was not one of those expected.
    fn unexpected(&self, expected: &str) -> JsonError {
        match self.text[self.pos..].chars().next() {
            None => self.error(format!("unexpected end of input, expected {expected}")),
            Some(found) => self.error(format!("unexpected {found:?}, expected {expected}")),
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.pos) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, byte: u8, expected: &str) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.unexpected(expected))
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), JsonError> {
        if self.text[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(self.error(format!("invalid literal, expected {word}")))
        }
    }

    /// Read an object key and its colon, leaving the cursor before the value.
    fn key(&mut self) -> Result<String, JsonError> {
        self.skip_whitespace();
        if self.peek() != Some(b'"') {
            return Err(self.unexpected("a string key"));
        }
        let key = self.string()?;
        self.skip_whitespace();
        self.expect(b':', "':'")?;
        Ok(key)
    }

    /// Read a string literal; the cursor is on its opening quote.
    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            // Copy the run up to the next quote, escape or control character
            // in one step; all three are ASCII, so the slice stays valid UTF-8.
            let run = self.bytes[self.pos..]
                .iter()
                .position(|&b| b == b'"' || b == b'\\' || b < 0x20)
                .unwrap_or(self.bytes.len() - self.pos);
            out.push_str(&self.text[self.pos..self.pos + run]);
            self.pos += run;

            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let decoded = match self.peek() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            self.pos += 1;
                            out.push(self.unicode_escape()?);
                            continue;
                        }
                        _ => return Err(self.unexpected("an escape such as \\n or \\u")),
                    };
                    self.pos += 1;
                    out.push(decoded);
                }
                Some(_) => return Err(self.error("control character in string")),
                None => return Err(self.error("unexpected end of input in string")),
            }
        }
    }

    /// Decode the digits after `\u`, joining a surrogate pair into one character.
    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let start = self.pos - 2;
        let first = self.hex4()?;
        let code = match first {
            0xD800..=0xDBFF => {
                if !self.text[self.pos..].starts_with("\\u") {
                    self.pos = start;
                    return Err(self.error("unpaired surrogate in \\u escape"));
                }
                self.pos += 2;
                let second = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    self.pos = start;
                    return Err(self.error("unpaired surrogate in \\u escape"));
                }
                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => {
                self.pos = start;
                return Err(self.error("unpaired surrogate in \\u escape"));
            }
            other => other,
        };
        Ok(char::from_u32(code).expect("surrogates were excluded"))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self.bytes.get(self.pos..self.pos + 4);
        match digits.and_then(|d| std::str::from_utf8(d).ok()) {
            Some(d) if d.bytes().all(|b| b.is_ascii_hexdigit()) => {
                self.pos += 4;
                Ok(u32::from_str_radix(d, 16).expect("four hex digits"))
            }
            _ => Err(self.error("expected four hex digits after \\u")),
        }
    }

    /// Read a number following the JSON grammar; the cursor is on its first byte.
    fn number(&mut self) -> Result<f64, JsonError> {
        let start = self.pos;
        let digits = |reader: &mut Self| {
            let from = reader.pos;
            while reader.peek().is_some_and(|b| b.is_ascii_digit()) {
                reader.pos += 1;
            }
            reader.pos > from
        };

        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return Err(self.unexpected("a digit")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !digits(self) {
                return Err(self.unexpected("a digit after '.'"));
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if !digits(self) {
                return Err(self.unexpected("a digit in the exponent"));
            }
        }

        let n: f64 = self.text[start..self.pos]
            .parse()
            .expect("the JSON number grammar is a subset of Rust's");
        if n.is_finite() {
            Ok(n)
        } else {
            self.pos = start;
            Err(self.error("number out of range"))
        }
    }
}

/// Parse one JSON value from `text`; only whitespace may surround it.
///
/// # Errors
///
/// Returns a [`JsonError`] with the line, column and character position of
/// the first fault, including an empty input and text after the value.
pub fn parse_json<T: LispContext>(text: &str) -> Result<LispExp<T>, JsonError> {
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        pos: 0,
    };
    let mut open: Vec<Open<T>> = Vec::new();
    // Intern the literal symbols once; each use is then a reference count.
    let (true_value, false_value, null_value) = (
        LispExp::t(),
        LispExp::symbol("false".into()),
        LispExp::symbol("null".into()),
    );

    loop {
        reader.skip_whitespace();
        let mut value = match reader.peek() {
            Some(b'{') => {
                reader.pos += 1;
                reader.skip_whitespace();
                if reader.peek() == Some(b'}') {
                    reader.pos += 1;
                    LispExp::map(HashMap::new())
                } else {
                    let key = reader.key()?;
                    open.push(Open::Object(HashMap::new(), key));
                    continue;
                }
            }
            Some(b'[') => {
                reader.pos += 1;
                reader.skip_whitespace();
                if reader.peek() == Some(b']') {
                    reader.pos += 1;
                    LispExp::vec(Vec::new())
                } else {
                    open.push(Open::Array(Vec::new()));
                    continue;
                }
            }
            Some(b'"') => LispExp::string(reader.string()?),
            Some(b't') => {
                reader.literal("true")?;
                true_value.clone()
            }
            Some(b'f') => {
                reader.literal("false")?;
                false_value.clone()
            }
            Some(b'n') => {
                reader.literal("null")?;
                null_value.clone()
            }
            Some(b'-' | b'0'..=b'9') => LispExp::number(reader.number()?),
            _ => return Err(reader.unexpected("a value")),
        };

        // Hand the finished value to the innermost open container, closing
        // containers for as long as their closing bracket follows.
        loop {
            reader.skip_whitespace();
            match open.last_mut() {
                None => {
                    return if reader.pos == text.len() {
                        Ok(value)
                    } else {
                        Err(reader.unexpected("the end of input"))
                    };
                }
                Some(Open::Array(items)) => {
                    items.push(value);
                    match reader.peek() {
                        Some(b',') => {
                            reader.pos += 1;
                            break;
                        }
                        Some(b']') => {
                            reader.pos += 1;
                            let Some(Open::Array(items)) = open.pop() else {
                                unreachable!("the innermost container is an array")
                            };
                            value = LispExp::vec(items);
                        }
                        _ => return Err(reader.unexpected("',' or ']'")),
                    }
                }
                Some(Open::Object(map, key)) => {
                    // A repeated key keeps its last value, as make-map does.
                    map.insert(std::mem::take(key), value);
                    match reader.peek() {
                        Some(b',') => {
                            reader.pos += 1;
                            *key = reader.key()?;
                            break;
                        }
                        Some(b'}') => {
                            reader.pos += 1;
                            let Some(Open::Object(map, _)) = open.pop() else {
                                unreachable!("the innermost container is an object")
                            };
                            value = LispExp::map(map);
                        }
                        _ => return Err(reader.unexpected("',' or '}'")),
                    }
                }
            }
        }
    }
}

/// Work left for [`write_json`].
enum Task<'a, T: LispContext> {
    Value(&'a LispExp<T>),
    Text(&'static str),
    Key(&'a str),
}

/// Write `exp` as compact JSON: maps as objects with sorted keys, vectors as
/// arrays, `t`, `false` and `null` as the literals of the same meaning.
///
/// # Errors
///
/// Returns the first part with no JSON meaning: `nil`, any other symbol, a
/// list, a non-finite number, or a function or other runtime object.
pub fn write_json<T: LispContext>(exp: &LispExp<T>) -> Result<String, LispExp<T>> {
    let mut out = String::new();
    let mut pending = vec![Task::Value(exp)];

    while let Some(task) = pending.pop() {
        match task {
            Task::Text(text) => out.push_str(text),
            Task::Key(key) => {
                write_string(&mut out, key);
                out.push(':');
            }
            Task::Value(value) => match value {
                LispExp::String(text) => write_string(&mut out, text),
                LispExp::Number(n) if n.is_finite() => write_number(&mut out, *n),
                LispExp::Symbol(name) if name.as_str() == "t" => out.push_str("true"),
                LispExp::Symbol(name) if name.as_str() == "false" => out.push_str("false"),
                LispExp::Symbol(name) if name.as_str() == "null" => out.push_str("null"),
                LispExp::Vector(items) => {
                    out.push('[');
                    pending.push(Task::Text("]"));
                    for (index, item) in items.iter().enumerate().rev() {
                        pending.push(Task::Value(item));
                        if index > 0 {
                            pending.push(Task::Text(","));
                        }
                    }
                }
                LispExp::Map(map) => {
                    let mut entries: Vec<_> = map.iter().collect();
                    entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
                    out.push('{');
                    pending.push(Task::Text("}"));
                    for (index, (key, item)) in entries.into_iter().enumerate().rev() {
                        pending.push(Task::Value(item));
                        pending.push(Task::Key(key));
                        if index > 0 {
                            pending.push(Task::Text(","));
                        }
                    }
                }
                other => return Err(other.clone()),
            },
        }
    }

    Ok(out)
}

/// Integers up to 2^53 without a fraction; others with the shortest digits
/// that read back the same, in exponent form when that is shorter.
fn write_number(out: &mut String, n: f64) {
    const EXACT_INTEGER_LIMIT: f64 = 9_007_199_254_740_992.0;
    if n.fract() == 0.0 && n.abs() < EXACT_INTEGER_LIMIT {
        out.push_str(&(n as i64).to_string());
    } else {
        // Debug prints finite floats in a form JSON accepts, such as 0.1,
        // 1e300 or 5e-324.
        out.push_str(&format!("{n:?}"));
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
