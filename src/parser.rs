//! ========================================================================== //
//!                    +----------------------------------------+
//!                    |  Parsing system for the lisp language  |
//!                    +----------------------------------------+
//! ========================================================================== //

use super::{LispContext, LispExp, Location, SourceMap};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) const MAX_READ_DEPTH: usize = 256;

/// The characters of the text, one of lookahead, together with the line and
/// column of the next one.
///
/// It has the two methods the lexer calls on the iterator it replaces, so the
/// state machine reads exactly as it did; only `next` learned to count.
struct Source<'source> {
    chars: std::iter::Peekable<std::str::Chars<'source>>,
    line: u32,
    column: u32,
}

// -----------------------------------------------------------------------------
//                  Structs necessary to the lexer to locate the code
// -----------------------------------------------------------------------------

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

/// What a parser in debug mode records its forms with.
struct Located {
    map: Arc<SourceMap>,
    file: Arc<str>,
}

// -----------------------------------------------------------------------------
//                      Structs necessary to the tokenizer
// -----------------------------------------------------------------------------

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

#[derive(Debug, PartialEq)]
pub enum ParserError {
    // Lexing
    UnbalancedRParen,
    UnbalancedRSquared,
    UnbalancedRBracket,
    NumberParseError(String),
    NumberInvadidChar(char),
    UnclosedString,
    // Parsing
    VoidExp,
    UnclosedList,
    UnclosedVector,
    UnclosedMap,
    InvalidMapKey,
    MapKeyMissingValue,
    UnexpectedToken(String),
    MalformedDottedList,
    TooDeep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

pub struct Parser<'source> {
    source: Source<'source>,
    token: String,
    current_token: Token,
    parens_stack: Vec<Token>,
    lexer_state: ParserLexerState,
    /// Debug mode: the table forms are recorded in, and the name of the text.
    /// `None` in the nominal mode, which then records nothing.
    located: Option<Located>,
    /// Where the source stood just after `current_token` was read. Kept only
    /// in debug mode.
    current_end: (u32, u32),
}

/// Convert a form produced by the reader into the data it denotes.
/// Runs once, at read time, on quoted structure only.
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

    /// A parser for debug mode: every list it reads is recorded in MAP as
    /// found in FILE, at the line and column of its `(`.
    ///
    /// FIRST_LINE is the number of the text's first line: 1 for a text read
    /// as it is, 0 for one a caller has prefixed with a line of its own -- the
    /// `(progn` `eval_file` wraps a file in -- so that the file's lines keep
    /// their numbers.
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

    pub(super) fn next_token(&mut self) -> Result<Option<Token>, ParserError> {
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
                // An escape inside a string.
                //
                // The named ones are the characters that cannot otherwise be
                // written: without `\n` a string cannot contain a newline at
                // all, so no module could build so much as a two-line
                // listing.
                //
                // Everything else keeps its backslash rather than being
                // rejected or silently eaten, because the regular expressions
                // the syntax rules are written out of are full of escapes that
                // mean something to `regex` and nothing here -- `\b`, `\s`,
                // `\.`. Passing those through is what lets a grammar be
                // written with single backslashes as well as doubled ones.
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

        // handle pending token
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
                            todo!("{:?}", self.token);
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

    /// Debug mode: where the `(` or `)` that is `current_token` was, as
    /// (line, column); `None` in the nominal mode.
    ///
    /// The lexer hands a parenthesis over the moment it consumes it, so the
    /// source stands one character past it -- on the same line, since a
    /// parenthesis is not a newline.
    fn paren_position(&self) -> Option<(u32, u32)> {
        self.located.as_ref()?;
        let (line, column) = self.current_end;
        Some((line, column - 1))
    }

    pub fn next<T: LispContext>(&mut self) -> Result<LispExp<T>, ParserError> {
        let mut frames: Vec<ReadFrame<T>> = Vec::new();
        let mut ready: Option<LispExp<T>> = None;

        if self.current_token == Token::Uninitialized {
            self.advance_token()?;
        }

        loop {
            // Loops until it can return a complete expression
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

            // Check that after the tail of a dotted list there is a close parens.
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

            // Checks that the key of a map is a string or a symbol.
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
