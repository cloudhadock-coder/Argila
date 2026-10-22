//! Lossless source model: values retain their original spelling and byte ranges.
use std::{fmt, ops::Range, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Object,
    Inherited,
    Inline,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    String(String),
    Number(f64),
    Ident(String),
    Set(Vec<String>),
    List(Vec<Value>),
    Collection(Vec<Vec<Property>>),
    Binary,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub name: String,
    pub value: Value,
    pub span: Range<usize>,
    pub line: usize,
    pub raw: String,
}

#[derive(Clone, Debug)]
pub struct Component {
    pub name: String,
    pub class: String,
    pub kind: ObjectKind,
    pub properties: Vec<Property>,
    pub children: Vec<Component>,
    pub span: Range<usize>,
    pub line: usize,
    pub end_line: usize,
}

impl Component {
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.properties
            .iter()
            .rev()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
    pub fn text(&self, name: &str) -> Option<&str> {
        match &self.property(name)?.value {
            Value::String(s) | Value::Ident(s) => Some(s),
            _ => None,
        }
    }
    /// Bounded metric for the preview. Original numeric values remain intact in `Value`.
    pub fn number(&self, name: &str, default: f32) -> f32 {
        match self.property(name).map(|p| &p.value) {
            Some(Value::Number(n)) if n.is_finite() => (*n as f32).clamp(-100_000., 100_000.),
            _ => default,
        }
    }
    pub fn boolean(&self, name: &str, default: bool) -> bool {
        self.text(name)
            .map_or(default, |s| !s.eq_ignore_ascii_case("False"))
    }
    pub fn at_line(&self, line: usize) -> &Component {
        self.children
            .iter()
            .find(|c| c.line <= line && line <= c.end_line)
            .map_or(self, |c| c.at_line(line))
    }
    pub fn find(&self, path: &str) -> Option<&Component> {
        let (head, tail) = path.split_once('/').unwrap_or((path, ""));
        if head != self.name {
            return None;
        }
        if tail.is_empty() {
            Some(self)
        } else {
            self.children.iter().find_map(|c| c.find(tail))
        }
    }
    pub fn path_at_line(&self, line: usize) -> String {
        match self
            .children
            .iter()
            .find(|c| c.line <= line && line <= c.end_line)
        {
            Some(c) => format!("{}/{}", self.name, c.path_at_line(line)),
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Form {
    pub source: Arc<str>,
    pub root: Component,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Line {}, column {}: {}",
            self.line + 1,
            self.column + 1,
            self.message
        )
    }
}

struct Parser<'a> {
    s: &'a str,
    pos: usize,
    line: usize,
    line_start: usize,
    nodes: usize,
}
impl<'a> Parser<'a> {
    fn err(&self, msg: &str) -> ParseError {
        ParseError {
            line: self.line,
            column: self.s[self.line_start..self.pos].chars().count(),
            message: msg.into(),
        }
    }
    fn peek(&self) -> Option<char> {
        self.s[self.pos..].chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.line_start = self.pos;
        }
        Some(c)
    }
    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }
    fn take(&mut self, c: char) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, c: char) -> Result<(), ParseError> {
        if self.take(c) {
            Ok(())
        } else {
            Err(self.err(&format!("Expected '{c}'")))
        }
    }
    fn word(&mut self) -> Result<String, ParseError> {
        self.ws();
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
        {
            self.bump();
        }
        if start == self.pos {
            Err(self.err("Expected identifier"))
        } else {
            Ok(self.s[start..self.pos].into())
        }
    }
    fn property(&mut self, depth: usize) -> Result<Property, ParseError> {
        self.ws();
        let start = self.pos;
        let line = self.line;
        let name = self.word()?;
        self.expect('=')?;
        self.ws();
        let raw_start = self.pos;
        let value = self.value(depth + 1)?;
        Ok(Property {
            name,
            value,
            span: start..self.pos,
            line,
            raw: self.s[raw_start..self.pos].into(),
        })
    }
    fn value(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > 128 {
            return Err(self.err("Value nesting limit (128) exceeded"));
        }
        self.ws();
        match self.peek() {
            Some('\'' | '#') => {
                let mut units = Vec::new();
                loop {
                    self.ws();
                    match self.peek() {
                        Some('\'') => {
                            self.bump();
                            loop {
                                match self.bump() {
                                    Some('\'') if self.peek() == Some('\'') => {
                                        self.bump();
                                        units.push(39);
                                    }
                                    Some('\'') => break,
                                    Some('\n' | '\r') | None => {
                                        return Err(self.err("Unterminated string"));
                                    }
                                    Some(c) => {
                                        let mut b = [0; 2];
                                        units.extend_from_slice(c.encode_utf16(&mut b));
                                    }
                                }
                            }
                        }
                        Some('#') => {
                            self.bump();
                            let hex = self.peek() == Some('$');
                            if hex {
                                self.bump();
                            }
                            let start = self.pos;
                            while self.peek().is_some_and(|c| {
                                if hex {
                                    c.is_ascii_hexdigit()
                                } else {
                                    c.is_ascii_digit()
                                }
                            }) {
                                self.bump();
                            }
                            let n = u16::from_str_radix(
                                &self.s[start..self.pos],
                                if hex { 16 } else { 10 },
                            )
                            .map_err(|_| self.err("Invalid character code"))?;
                            units.push(n);
                        }
                        _ => break,
                    }
                    // Whitespace is allowed between string fragments, including wrapped lines.
                    let saved = (self.pos, self.line, self.line_start);
                    self.ws();
                    if self.peek() == Some('+') {
                        self.bump();
                        self.ws();
                        if !matches!(self.peek(), Some('\'' | '#')) {
                            return Err(self.err("Expected string after '+'"));
                        }
                    } else if self.line != saved.1 || !matches!(self.peek(), Some('\'' | '#')) {
                        (self.pos, self.line, self.line_start) = saved;
                        break;
                    }
                }
                Ok(Value::String(String::from_utf16_lossy(&units)))
            }
            Some('{') => {
                self.bump();
                let mut digits = 0;
                loop {
                    match self.bump() {
                        Some('}') => break,
                        Some(c) if c.is_whitespace() => {}
                        Some(c) if c.is_ascii_hexdigit() => digits += 1,
                        Some(_) => return Err(self.err("Invalid hexadecimal binary data")),
                        None => return Err(self.err("Unterminated binary data")),
                    }
                }
                if digits % 2 != 0 {
                    return Err(self.err("Odd number of binary hex digits"));
                }
                Ok(Value::Binary)
            }
            Some('[') => {
                self.bump();
                let mut values = Vec::new();
                if !self.take(']') {
                    loop {
                        values.push(self.word()?);
                        if self.take(']') {
                            break;
                        }
                        self.expect(',')?;
                    }
                }
                Ok(Value::Set(values))
            }
            Some('(') => {
                self.bump();
                let mut values = Vec::new();
                while !self.take(')') {
                    values.push(self.value(depth + 1)?);
                    self.take(',');
                }
                Ok(Value::List(values))
            }
            Some('<') => {
                self.bump();
                let mut items = Vec::new();
                while !self.take('>') {
                    if !self.word()?.eq_ignore_ascii_case("item") {
                        return Err(self.err("Expected collection item"));
                    }
                    // Optional collection item index.
                    if self.take('[') {
                        self.value(depth + 1)?;
                        self.expect(']')?;
                    }
                    let mut props = Vec::new();
                    loop {
                        self.ws();
                        let saved = (self.pos, self.line, self.line_start);
                        if self.word()?.eq_ignore_ascii_case("end") {
                            break;
                        }
                        (self.pos, self.line, self.line_start) = saved;
                        props.push(self.property(depth + 1)?);
                    }
                    items.push(props);
                }
                Ok(Value::Collection(items))
            }
            Some(c) if c.is_ascii_digit() || matches!(c, '-' | '+' | '$') => {
                let start = self.pos;
                self.bump();
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
                {
                    self.bump();
                }
                let raw = &self.s[start..self.pos];
                let n = if let Some(hex) = raw.strip_prefix('$') {
                    u64::from_str_radix(hex, 16).ok().map(|n| n as f64)
                } else {
                    raw.parse::<f64>().ok()
                };
                n.filter(|n| n.is_finite())
                    .map(Value::Number)
                    .ok_or_else(|| self.err("Invalid number"))
            }
            Some(_) => self.word().map(Value::Ident),
            None => Err(self.err("Expected property value")),
        }
    }
    fn component(&mut self, depth: usize) -> Result<Component, ParseError> {
        if depth > 128 || self.nodes >= 100_000 {
            return Err(self.err("Component safety limit exceeded"));
        }
        self.nodes += 1;
        self.ws();
        let start = self.pos;
        let line = self.line;
        let kind = match self.word()?.to_ascii_lowercase().as_str() {
            "object" => ObjectKind::Object,
            "inherited" => ObjectKind::Inherited,
            "inline" => ObjectKind::Inline,
            _ => return Err(self.err("Expected object, inherited or inline")),
        };
        let name = self.word()?;
        let class = if self.take(':') {
            self.word()?
        } else if kind == ObjectKind::Inherited {
            "(inherited)".into()
        } else {
            return Err(self.err("Expected component class"));
        };
        if self.take('[') {
            self.value(0)?;
            self.expect(']')?;
        }
        let mut properties = Vec::new();
        let mut children = Vec::new();
        loop {
            self.ws();
            let saved = (self.pos, self.line, self.line_start);
            let word = self.word()?.to_ascii_lowercase();
            if word == "end" {
                break;
            }
            (self.pos, self.line, self.line_start) = saved;
            if matches!(word.as_str(), "object" | "inherited" | "inline") {
                children.push(self.component(depth + 1)?);
            } else {
                properties.push(self.property(0)?);
            }
        }
        Ok(Component {
            name,
            class,
            kind,
            properties,
            children,
            span: start..self.pos,
            line,
            end_line: self.line,
        })
    }
}

pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(b"TPF0")
}

pub fn parse(source: &str) -> Result<Form, ParseError> {
    let mut p = Parser {
        s: source,
        pos: 0,
        line: 0,
        line_start: 0,
        nodes: 0,
    };
    if p.peek() == Some('\u{feff}') {
        p.bump();
    }
    let root = p.component(0)?;
    p.ws();
    if p.pos != source.len() {
        return Err(p.err("Unexpected text after root component"));
    }
    Ok(Form {
        source: Arc::from(source),
        root,
    })
}
