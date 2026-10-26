//! Tokenization and iterative reading of Lisp syntax and quoted data.

use super::{LispContext, LispExp, Location, SourceMap};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) const MAX_READ_DEPTH: usize = 256;

/// Character lookahead with the line and column of the next character.
struct Source<'source> {
    chars: std::iter::Peekable<std::str::Chars<'source>>,
    line: u32,
    column: u32,
}

impl<'source> Source<'source> {
    fn new(text: &'source str, first_line: u32) -> Self {
        Self {
            chars: text.chars().peekable(),
            line: first_line,
            column: 1,
        }
    }

    fn peek(&mut self) -> Option<&char> {
        self.chars.peek()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }
}

/// Source-map destination and source name for located forms.
struct Located {
    map: Arc<SourceMap>,
    file: Arc<str>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Token {
    Uninitialized,
    Void,
    LParen,
    RParen,
    LSquared,
    RSquared,
    LBracket,
    RBracket,
    Quote,
    BackQuote,
    Comma,
    CommaAt,
    Dot,
    Number(f64),
    String(String),
    Symbol(String),
}

enum ParserLexerState {
    Default,
    InComment,
    InSymbol,
    InString,
    InStringSlash,
    InNumber,
    InNumberMinusStart,
    InNumberAfterDot,
    InDotStart,
}

/// Lexical or structural reader failure.
///
/// `VoidExp` covers both end of input and an incomplete reader prefix.
#[derive(Debug, PartialEq)]
pub enum ParserError {
    /// A closing `)` has no matching opening `(`.
    UnbalancedRParen,
    /// A closing `]` has no matching opening `[`.
    UnbalancedRSquared,
    /// A closing `}` has no matching opening `{`.
    UnbalancedRBracket,
    /// The contained token cannot be read as a number.
    NumberParseError(String),
    /// Invalid character in a numeric token; retained for compatibility.
    NumberInvadidChar(char),
    /// Input ended inside a string or a string escape.
    UnclosedString,
    /// No expression is available, including end of input or a dangling reader prefix.
    VoidExp,
    /// Input ended before a list was closed.
    UnclosedList,
    /// Input ended before a vector was closed.
    UnclosedVector,
    /// Input ended before a map was closed.
    UnclosedMap,
    /// A map key is neither a symbol nor a string.
    InvalidMapKey,
    /// A map closes with a key that has no value.
    MapKeyMissingValue,
    /// The contained token is not valid at the current reader position.
    UnexpectedToken(String),
    /// A dotted list has a missing or additional tail expression or closing delimiter.
    MalformedDottedList,
    /// Reading would exceed the maximum of 256 nested reader frames.
    TooDeep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
/// Quote keeps descendant lists as data; unquote restores code inside a template.
enum Mode {
    Code,
    Data,
    Template,
}

enum Tail<T: LispContext> {
    Proper,
    Missing,
    Value(LispExp<T>),
}

enum Kind<T: LispContext> {
    List {
        items: Vec<LispExp<T>>,
        tail: Tail<T>,
        opened: Option<(u32, u32)>,
    },
    Vector(Vec<LispExp<T>>),
    Map {
        items: HashMap<String, LispExp<T>>,
        key: Option<String>,
    },
    Prefix(&'static str),
}

struct ReadFrame<T: LispContext> {
    mode: Mode,
    kind: Kind<T>,
}

impl<T: LispContext> ReadFrame<T> {
    fn child_mode(&self) -> Mode {
        if self.mode == Mode::Data {
            return Mode::Data;
        }

        let head = match &self.kind {
            Kind::List { items, .. } => match items.first() {
                Some(LispExp::Symbol(name)) => Some(name.as_str()),
                _ => None,
            },
            Kind::Prefix(name) => Some(*name),
            _ => None,
        };

        match head {
            Some("quote") if self.mode == Mode::Code => Mode::Data,
            Some("backquote") => Mode::Template,
            Some("unquote") | Some("unquote-splicing") => Mode::Code,
            _ => self.mode,
        }
    }

    fn eof_error(&self) -> ParserError {
        match &self.kind {
            Kind::List {
                tail: Tail::Missing,
                ..
            } => ParserError::UnexpectedToken(".".into()),

            Kind::List {
                tail: Tail::Value(_),
                ..
            } => ParserError::MalformedDottedList,

            Kind::List { .. } => ParserError::UnclosedList,
            Kind::Vector(_) => ParserError::UnclosedVector,
            Kind::Map { .. } => ParserError::UnclosedMap,
            Kind::Prefix(_) => ParserError::VoidExp,
        }
    }
}

/// A stateful reader over borrowed Lisp source.
///
/// Call [`Self::next`] repeatedly to read successive expressions. Ordinary code
/// lists become [`LispExp::Form`] nodes; quoted lists and dotted lists become
/// cons data. No evaluation fuel is charged while reading.
pub struct Parser<'source> {
    source: Source<'source>,
    token: String,
    current_token: Token,
    parens_stack: Vec<Token>,
    lexer_state: ParserLexerState,
    /// Optional source-map recording and source name.
    located: Option<Located>,
    /// Position just after the current token; updated only while recording locations.
    current_end: (u32, u32),
}

/// Convert syntax forms to proper data lists recursively.
///
/// Recurse through vector and map contents, preserving those container types.
/// Other values, including existing cons lists, are cloned unchanged.
pub fn form_to_data<T: LispContext>(exp: &LispExp<T>) -> LispExp<T> {
    match exp {
        LispExp::Form(items) => LispExp::proper_list(items.iter().map(form_to_data).collect()),
        LispExp::Vector(items) => LispExp::vec(items.iter().map(form_to_data).collect()),
        LispExp::Map(m) => LispExp::map(
            m.iter()
                .map(|(k, v)| (k.clone(), form_to_data(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

impl<'source> Parser<'source> {
    /// Create a reader without source-location tracking.
    pub fn new(source: &'source str) -> Self {
        Self {
            source: Source::new(source, 1),
            token: String::new(),
            current_token: Token::Uninitialized,
            parens_stack: Vec::new(),
            lexer_state: ParserLexerState::Default,
            located: None,
            current_end: (1, 1),
        }
    }

    /// Create a parser that records list locations in `map` under `file`.
    ///
    /// `first_line` is the starting line number, normally 1. Use 0 when a caller
    /// prefixes the input with one synthetic line to preserve original numbering.
    pub fn with_source_map(
        source: &'source str,
        map: Arc<SourceMap>,
        file: &str,
        first_line: u32,
    ) -> Self {
        let mut parser = Self::new(source);
        parser.source = Source::new(source, first_line);
        parser.located = Some(Located {
            map,
            file: file.into(),
        });
        parser
    }

    /// Discard cached state belonging to a failed expression.
    #[cold]
    fn reset_after_error(&mut self) {
        self.token.clear();
        self.lexer_state = ParserLexerState::Default;
        self.current_token = Token::Uninitialized;
        self.parens_stack.clear();
    }

    /// Consume the remainder of a malformed numeric token. Keep its delimiter
    /// available to the normal lexer, including a following comment marker.
    #[cold]
    fn discard_numeric_tail(&mut self) {
        while let Some(&c) = self.source.peek() {
            if matches!(
                c,
                ' ' | '\t' | '\n' | '\r' | '(' | ')' | '[' | ']' | '{' | '}' | ';'
            ) {
                break;
            }
            self.source.next();
        }
    }

    pub(super) fn next_token(&mut self) -> Result<Option<Token>, ParserError> {
        match self.lex_next_token() {
            Ok(token) => Ok(token),
            Err(err) => {
                match &err {
                    ParserError::NumberParseError(_) => self.discard_numeric_tail(),
                    ParserError::UnbalancedRParen
                    | ParserError::UnbalancedRSquared
                    | ParserError::UnbalancedRBracket => {
                        // These branches detected, but did not consume, the delimiter.
                        self.source.next();
                    }
                    _ => {}
                }
                self.reset_after_error();
                Err(err)
            }
        }
    }

    fn lex_next_token(&mut self) -> Result<Option<Token>, ParserError> {
        while let Some(c) = self.source.peek() {
            match self.lexer_state {
                ParserLexerState::Default => match c {
                    ';' => {
                        self.lexer_state = ParserLexerState::InComment;
                    }
                    '\'' => {
                        self.source.next();
                        return Ok(Some(Token::Quote));
                    }
                    '`' => {
                        self.source.next();
                        return Ok(Some(Token::BackQuote));
                    }
                    ',' => {
                        self.source.next();
                        if self.source.peek() == Some(&'@') {
                            self.source.next();
                            return Ok(Some(Token::CommaAt));
                        }
                        return Ok(Some(Token::Comma));
                    }
                    '(' => {
                        self.parens_stack.push(Token::LParen);
                        self.source.next();
                        return Ok(Some(Token::LParen));
                    }
                    ')' => {
                        if Some(Token::LParen) == self.parens_stack.pop() {
                            self.source.next();
                            return Ok(Some(Token::RParen));
                        } else {
                            return Err(ParserError::UnbalancedRParen);
                        }
                    }
                    '[' => {
                        self.parens_stack.push(Token::LSquared);
                        self.source.next();
                        return Ok(Some(Token::LSquared));
                    }
                    ']' => {
                        if Some(Token::LSquared) == self.parens_stack.pop() {
                            self.source.next();
                            return Ok(Some(Token::RSquared));
                        } else {
                            return Err(ParserError::UnbalancedRSquared);
                        }
                    }
                    '{' => {
                        self.parens_stack.push(Token::LBracket);
                        self.source.next();
                        return Ok(Some(Token::LBracket));
                    }
                    '}' => {
                        if Some(Token::LBracket) == self.parens_stack.pop() {
                            self.source.next();
                            return Ok(Some(Token::RBracket));
                        } else {
                            return Err(ParserError::UnbalancedRBracket);
                        }
                    }
                    ' ' | '\t' | '\n' | '\r' => {}
                    '"' => {
                        self.lexer_state = ParserLexerState::InString;
                    }
                    '-' => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InNumberMinusStart;
                    }
                    '0'..='9' => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InNumber;
                    }
                    '.' => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InDotStart;
                    }
                    _ => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InSymbol;
                    }
                },
                ParserLexerState::InComment => match c {
                    '\n' => {
                        self.lexer_state = ParserLexerState::Default;
                    }
                    _ => {}
                },
                ParserLexerState::InSymbol => match c {
                    ';' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        self.lexer_state = ParserLexerState::InComment;
                        self.source.next();
                        return Ok(Some(Token::Symbol(token_string)));
                    }
                    ' ' | '\t' | '\n' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        self.lexer_state = ParserLexerState::Default;
                        self.source.next();
                        return Ok(Some(Token::Symbol(token_string)));
                    }
                    '(' | '[' | '{' | ')' | ']' | '}' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        self.lexer_state = ParserLexerState::Default;
                        return Ok(Some(Token::Symbol(token_string)));
                    }
                    '\r' => {}
                    _ => {
                        self.token.push(*c);
                    }
                },
                ParserLexerState::InString => match c {
                    '"' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        self.lexer_state = ParserLexerState::Default;
                        self.source.next();
                        return Ok(Some(Token::String(token_string)));
                    }
                    '\\' => {
                        self.lexer_state = ParserLexerState::InStringSlash;
                    }
                    _ => {
                        self.token.push(*c);
                    }
                },
                // Decode named escapes; preserve other backslashes for downstream regex parsers.
                ParserLexerState::InStringSlash => match c {
                    '"' | '\\' => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InString;
                    }
                    'n' => {
                        self.token.push('\n');
                        self.lexer_state = ParserLexerState::InString;
                    }
                    't' => {
                        self.token.push('\t');
                        self.lexer_state = ParserLexerState::InString;
                    }
                    'r' => {
                        self.token.push('\r');
                        self.lexer_state = ParserLexerState::InString;
                    }
                    'e' => {
                        self.token.push('\u{1b}');
                        self.lexer_state = ParserLexerState::InString;
                    }
                    '0' => {
                        self.token.push('\0');
                        self.lexer_state = ParserLexerState::InString;
                    }
                    _ => {
                        self.token.push('\\');
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InString;
                    }
                },
                ParserLexerState::InNumber => match c {
                    ';' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        if let Ok(number) = token_string.parse() {
                            self.lexer_state = ParserLexerState::InComment;
                            return Ok(Some(Token::Number(number)));
                        } else {
                            return Err(ParserError::NumberParseError(token_string));
                        }
                    }
                    '0'..='9' => {
                        self.token.push(*c);
                    }
                    '.' => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InNumberAfterDot;
                    }
                    '(' | '[' | '{' | ')' | ']' | '}' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        if let Ok(number) = token_string.parse() {
                            self.lexer_state = ParserLexerState::Default;
                            return Ok(Some(Token::Number(number)));
                        } else {
                            return Err(ParserError::NumberParseError(token_string));
                        }
                    }
                    ' ' | '\t' | '\n' | '\r' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        if let Ok(number) = token_string.parse() {
                            self.lexer_state = ParserLexerState::Default;
                            self.source.next();
                            return Ok(Some(Token::Number(number)));
                        } else {
                            return Err(ParserError::NumberParseError(token_string));
                        }
                    }
                    _ => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InSymbol;
                    }
                },
                ParserLexerState::InNumberMinusStart => match c {
                    ';' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        self.lexer_state = ParserLexerState::InComment;
                        return Ok(Some(Token::Symbol(token_string)));
                    }
                    '0'..='9' => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InNumber;
                    }
                    '.' => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InNumberAfterDot;
                    }
                    '(' | '[' | '{' | ')' | ']' | '}' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        self.lexer_state = ParserLexerState::Default;
                        return Ok(Some(Token::Symbol(token_string)));
                    }
                    ' ' | '\t' | '\n' | '\r' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        self.lexer_state = ParserLexerState::Default;
                        return Ok(Some(Token::Symbol(token_string)));
                    }
                    _ => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InSymbol;
                    }
                },
                ParserLexerState::InNumberAfterDot => match c {
                    ';' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        if let Ok(number) = token_string.parse() {
                            self.lexer_state = ParserLexerState::InComment;
                            return Ok(Some(Token::Number(number)));
                        } else {
                            return Err(ParserError::NumberParseError(token_string));
                        }
                    }
                    '0'..='9' => {
                        self.token.push(*c);
                    }
                    '(' | '[' | '{' | ')' | ']' | '}' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        if let Ok(number) = token_string.parse() {
                            self.lexer_state = ParserLexerState::Default;
                            return Ok(Some(Token::Number(number)));
                        } else {
                            return Err(ParserError::NumberParseError(token_string));
                        }
                    }
                    ' ' | '\t' | '\n' | '\r' => {
                        let mut token_string = String::new();
                        core::mem::swap(&mut token_string, &mut self.token);
                        if let Ok(number) = token_string.parse() {
                            self.lexer_state = ParserLexerState::Default;
                            self.source.next();
                            return Ok(Some(Token::Number(number)));
                        } else {
                            return Err(ParserError::NumberParseError(token_string));
                        }
                    }
                    _ => {
                        self.token.push(*c);
                        return Err(ParserError::NumberParseError(self.token.clone()));
                    }
                },

                ParserLexerState::InDotStart => match c {
                    '0'..'9' => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InNumberAfterDot;
                    }
                    ';' => {
                        self.token.clear();
                        self.lexer_state = ParserLexerState::InComment;
                        return Ok(Some(Token::Dot));
                    }
                    '(' | '[' | '{' | ')' | ']' | '}' => {
                        self.token.clear();
                        self.lexer_state = ParserLexerState::Default;
                        return Ok(Some(Token::Dot));
                    }
                    ' ' | '\t' | '\n' | '\r' => {
                        self.token.clear();
                        self.lexer_state = ParserLexerState::Default;
                        return Ok(Some(Token::Dot));
                    }
                    _ => {
                        self.token.push(*c);
                        self.lexer_state = ParserLexerState::InSymbol;
                    }
                },
            }
            self.source.next();
        }

        match self.lexer_state {
            ParserLexerState::Default => {
                if !self.token.is_empty() {
                    match self.token.as_str() {
                        ")" => {
                            if Some(Token::LParen) == self.parens_stack.pop() {
                                self.source.next();
                                return Ok(Some(Token::RParen));
                            } else {
                                return Err(ParserError::UnbalancedRParen);
                            }
                        }
                        "]" => {
                            if Some(Token::LSquared) == self.parens_stack.pop() {
                                self.source.next();
                                return Ok(Some(Token::RSquared));
                            } else {
                                return Err(ParserError::UnbalancedRSquared);
                            }
                        }
                        "}" => {
                            if Some(Token::LBracket) == self.parens_stack.pop() {
                                self.source.next();
                                return Ok(Some(Token::RBracket));
                            } else {
                                return Err(ParserError::UnbalancedRBracket);
                            }
                        }
                        _ => {
                            return Err(ParserError::UnexpectedToken(std::mem::take(
                                &mut self.token,
                            )));
                        }
                    }
                } else {
                    return Ok(None);
                }
            }
            ParserLexerState::InComment => {
                return Ok(None);
            }
            ParserLexerState::InSymbol | ParserLexerState::InNumberMinusStart => {
                self.lexer_state = ParserLexerState::Default;
                let mut token_string = String::new();
                core::mem::swap(&mut token_string, &mut self.token);
                self.source.next();
                return Ok(Some(Token::Symbol(token_string)));
            }
            ParserLexerState::InNumber | ParserLexerState::InNumberAfterDot => {
                let mut token_string = String::new();
                core::mem::swap(&mut token_string, &mut self.token);
                if let Ok(number) = token_string.parse() {
                    self.lexer_state = ParserLexerState::Default;
                    self.source.next();
                    return Ok(Some(Token::Number(number)));
                } else {
                    return Err(ParserError::NumberParseError(token_string));
                }
            }
            ParserLexerState::InDotStart => {
                self.lexer_state = ParserLexerState::Default;

                self.token.clear();
                return Ok(Some(Token::Dot));
            }
            ParserLexerState::InString | ParserLexerState::InStringSlash => {
                return Err(ParserError::UnclosedString);
            }
        }
    }

    fn advance_token(&mut self) -> Result<(), ParserError> {
        self.current_token = if let Some(token) = self.next_token()? {
            token
        } else {
            Token::Void
        };
        if self.located.is_some() {
            self.current_end = (self.source.line, self.source.column);
        }
        Ok(())
    }

    /// Position of the current delimiter when source tracking is enabled.
    /// The lexer has already consumed it, so subtract one column.
    fn paren_position(&self) -> Option<(u32, u32)> {
        self.located.as_ref()?;
        let (line, column) = self.current_end;
        Some((line, column - 1))
    }

    /// Read the next expression.
    ///
    /// # Errors
    ///
    /// Returns `VoidExp` when no expression is available. This includes both
    /// clean end of input and a dangling quote or other reader prefix.
    /// Malformed input returns a lexical or structural error; nesting beyond
    /// 256 reader frames returns `TooDeep`. After an error, cached state for the
    /// failed expression is discarded. Malformed numeric tokens are skipped to
    /// their next delimiter; general structural resynchronization is not guaranteed.
    pub fn next<T: LispContext>(&mut self) -> Result<LispExp<T>, ParserError> {
        let result = self.read_next();
        if result.is_err() {
            self.reset_after_error();
        }
        result
    }

    fn read_next<T: LispContext>(&mut self) -> Result<LispExp<T>, ParserError> {
        let mut frames: Vec<ReadFrame<T>> = Vec::new();
        let mut ready: Option<LispExp<T>> = None;

        if self.current_token == Token::Uninitialized {
            self.advance_token()?;
        }

        loop {
            if let Some(value) = ready.take() {
                let Some(frame) = frames.last_mut() else {
                    return Ok(value);
                };

                match &mut frame.kind {
                    Kind::Prefix(name) => {
                        let items = vec![LispExp::symbol((*name).into()), value];
                        ready = Some(if frame.mode == Mode::Data {
                            LispExp::proper_list(items)
                        } else {
                            LispExp::form(items)
                        });

                        let _ = frames.pop();
                    }

                    Kind::List { items, tail, .. } => match tail {
                        Tail::Proper => items.push(value),
                        Tail::Missing => *tail = Tail::Value(value),
                        Tail::Value(_) => {
                            return Err(ParserError::MalformedDottedList);
                        }
                    },

                    Kind::Vector(items) => items.push(value),

                    Kind::Map { items, key } => {
                        let key = key.take().ok_or(ParserError::InvalidMapKey)?;

                        items.insert(key, value);
                    }
                }

                continue;
            }

            if matches!(
                frames.last(),
                Some(ReadFrame {
                    kind: Kind::List {
                        tail: Tail::Value(_),
                        ..
                    },
                    ..
                })
            ) && self.current_token != Token::RParen
            {
                return Err(ParserError::MalformedDottedList);
            }

            if let Some(ReadFrame {
                kind: Kind::Map { key, .. },
                ..
            }) = frames.last_mut()
            {
                if key.is_none() {
                    match &self.current_token {
                        Token::Symbol(name) | Token::String(name) => {
                            *key = Some(name.clone());
                            self.advance_token()?;
                            continue;
                        }

                        Token::RBracket | Token::Void => {}

                        _ => return Err(ParserError::InvalidMapKey),
                    }
                }
            }

            let mode = frames.last().map_or(Mode::Code, ReadFrame::child_mode);

            match self.current_token.clone() {
                Token::Void => {
                    return Err(frames
                        .last()
                        .map_or(ParserError::VoidExp, ReadFrame::eof_error));
                }

                Token::Symbol(name) => {
                    self.advance_token()?;
                    ready = Some(LispExp::symbol(name));
                }

                Token::String(value) => {
                    self.advance_token()?;
                    ready = Some(LispExp::string(value));
                }

                Token::Number(value) => {
                    self.advance_token()?;
                    ready = Some(LispExp::number(value));
                }

                Token::LParen
                | Token::LSquared
                | Token::LBracket
                | Token::Quote
                | Token::BackQuote
                | Token::Comma
                | Token::CommaAt => {
                    if frames.len() >= MAX_READ_DEPTH {
                        return Err(ParserError::TooDeep);
                    }

                    let kind = match self.current_token {
                        Token::LParen => Kind::List {
                            items: Vec::new(),
                            tail: Tail::Proper,
                            opened: self.paren_position(),
                        },

                        Token::LSquared => Kind::Vector(Vec::new()),

                        Token::LBracket => Kind::Map {
                            items: HashMap::new(),
                            key: None,
                        },

                        Token::Quote => Kind::Prefix("quote"),
                        Token::BackQuote => Kind::Prefix("backquote"),
                        Token::Comma => Kind::Prefix("unquote"),
                        Token::CommaAt => Kind::Prefix("unquote-splicing"),

                        _ => return Err(ParserError::VoidExp),
                    };

                    frames.push(ReadFrame { mode, kind });
                    self.advance_token()?;
                }

                Token::Dot => {
                    match frames.last_mut() {
                        Some(ReadFrame {
                            kind: Kind::List { items, tail, .. },
                            ..
                        }) if !items.is_empty() && matches!(&*tail, Tail::Proper) => {
                            *tail = Tail::Missing;
                        }

                        _ => return Err(ParserError::UnexpectedToken(".".into())),
                    }

                    self.advance_token()?;
                }

                close @ (Token::RParen | Token::RSquared | Token::RBracket) => {
                    let spelling = match close {
                        Token::RParen => ")",
                        Token::RSquared => "]",
                        _ => "}",
                    };

                    let closed = self.paren_position();

                    let frame = frames
                        .pop()
                        .ok_or_else(|| ParserError::UnexpectedToken(spelling.into()))?;

                    let value = match frame.kind {
                        Kind::List {
                            items,
                            tail,
                            opened,
                        } if close == Token::RParen => {
                            let value = match tail {
                                Tail::Missing => {
                                    return Err(ParserError::UnexpectedToken(".".into()));
                                }

                                Tail::Value(tail) => LispExp::improper_list(items, tail),

                                Tail::Proper if frame.mode == Mode::Data => {
                                    LispExp::proper_list(items)
                                }

                                Tail::Proper => LispExp::form(items),
                            };

                            if let (
                                Some((line, column)),
                                Some((end_line, end_column)),
                                Some(located),
                                LispExp::Form(form),
                            ) = (opened, closed, &self.located, &value)
                            {
                                located.map.record(
                                    form,
                                    Location {
                                        file: located.file.clone(),
                                        line,
                                        column,
                                        end_line,
                                        end_column,
                                    },
                                );
                            }

                            value
                        }

                        Kind::Vector(items) if close == Token::RSquared => LispExp::vec(items),

                        Kind::Map { items, key } if close == Token::RBracket => {
                            if key.is_some() {
                                return Err(ParserError::MapKeyMissingValue);
                            }

                            LispExp::map(items)
                        }

                        _ => return Err(ParserError::UnexpectedToken(spelling.into())),
                    };

                    self.advance_token()?;
                    ready = Some(value);
                }

                Token::Uninitialized => self.advance_token()?,
            }
        }
    }
}
