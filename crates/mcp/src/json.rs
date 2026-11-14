//! A small JSON reader and writer for the MCP transport (RFC 8259).
//!
//! Input is hostile: nesting depth, string and container sizes are capped, and
//! numbers that do not fit an `f64` are refused rather than guessed.

use std::fmt::Write as _;

/// Deepest nesting read.
pub const MAX_DEPTH: usize = 64;
/// Most items in one array or object.
pub const MAX_ITEMS: usize = 100_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    /// Members in the order they were written.
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn obj(members: Vec<(&str, Value)>) -> Value {
        Value::Obj(members.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }

    /// The value as tool-argument text: strings as they are, numbers and
    /// booleans written out, arrays of scalars joined with commas.
    pub fn as_argument(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.clone()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Num(n) => Some(number(*n)),
            Value::Arr(items) => {
                let parts: Option<Vec<String>> = items
                    .iter()
                    .map(|v| match v {
                        Value::Arr(_) | Value::Obj(_) => None,
                        other => other.as_argument(),
                    })
                    .collect();
                parts.map(|p| p.join(","))
            }
            Value::Null | Value::Obj(_) => None,
        }
    }
}

fn number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else if n.is_finite() {
        format!("{n}")
    } else {
        "null".into()
    }
}

/// Write a value as compact JSON.
pub fn write(v: &Value) -> String {
    let mut out = String::new();
    write_into(v, &mut out);
    out
}

fn write_into(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) => out.push_str(&number(*n)),
        Value::Str(s) => write_str(s, out),
        Value::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_into(item, out);
            }
            out.push(']');
        }
        Value::Obj(members) => {
            out.push('{');
            for (i, (k, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write_into(item, out);
            }
            out.push('}');
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Read one JSON value; trailing text other than whitespace is an error.
pub fn parse(text: &str) -> Result<Value, &'static str> {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err("text after the JSON value");
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        if self.s.get(self.i..self.i + lit.len()) == Some(lit) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, &'static str> {
        if depth > MAX_DEPTH {
            return Err("JSON nested too deeply");
        }
        self.ws();
        match self.peek() {
            None => Err("unexpected end of JSON"),
            Some(b'n') if self.eat(b"null") => Ok(Value::Null),
            Some(b't') if self.eat(b"true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat(b"false") => Ok(Value::Bool(false)),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat(b"]") {
                    return Ok(Value::Arr(items));
                }
                loop {
                    if items.len() >= MAX_ITEMS {
                        return Err("a JSON array is too long");
                    }
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat(b",") {
                        continue;
                    }
                    if self.eat(b"]") {
                        return Ok(Value::Arr(items));
                    }
                    return Err("expected , or ] in a JSON array");
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut members = Vec::new();
                self.ws();
                if self.eat(b"}") {
                    return Ok(Value::Obj(members));
                }
                loop {
                    if members.len() >= MAX_ITEMS {
                        return Err("a JSON object is too long");
                    }
                    self.ws();
                    if self.peek() != Some(b'"') {
                        return Err("expected a member name in a JSON object");
                    }
                    let k = self.string()?;
                    self.ws();
                    if !self.eat(b":") {
                        return Err("expected : in a JSON object");
                    }
                    let v = self.value(depth + 1)?;
                    members.push((k, v));
                    self.ws();
                    if self.eat(b",") {
                        continue;
                    }
                    if self.eat(b"}") {
                        return Ok(Value::Obj(members));
                    }
                    return Err("expected , or } in a JSON object");
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err("not a JSON value"),
        }
    }

    fn number(&mut self) -> Result<Value, &'static str> {
        let start = self.i;
        while matches!(self.peek(), Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
            self.i += 1;
        }
        let text = std::str::from_utf8(self.s.get(start..self.i).unwrap_or(&[])).unwrap_or("");
        match text.parse::<f64>() {
            Ok(n) if n.is_finite() && !text.starts_with('+') && !text.ends_with('.') => {
                Ok(Value::Num(n))
            }
            _ => Err("a bad JSON number"),
        }
    }

    fn hex4(&mut self) -> Result<u32, &'static str> {
        let digits = self.s.get(self.i..self.i + 4).ok_or("a cut-off \\u escape")?;
        let text = std::str::from_utf8(digits).map_err(|_| "a bad \\u escape")?;
        let v = u32::from_str_radix(text, 16).map_err(|_| "a bad \\u escape")?;
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, &'static str> {
        // at the opening quote
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek() else { return Err("a JSON string is not closed") };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(e) = self.peek() else { return Err("a cut-off escape") };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                if !self.eat(b"\\u") {
                                    return Err("a lone surrogate in a JSON string");
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err("a bad surrogate pair in a JSON string");
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(code).ok_or("a bad \\u escape")?
                        }
                        _ => return Err("a bad escape in a JSON string"),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c if c < 0x20 => return Err("a control character in a JSON string"),
                c => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| "a JSON string is not UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let text = r#"{"a":[1,2.5,-3e2,true,false,null],"b":"x\"y\\z\n\u00e9\ud83d\ude00","c":{}}"#;
        let v = parse(text);
        assert!(v.is_ok(), "{v:?}");
        let Ok(v) = v else { return };
        assert_eq!(v.get("b").and_then(Value::as_str), Some("x\"y\\z\né😀"));
        let again = parse(&write(&v));
        assert_eq!(again, Ok(v));
    }

    #[test]
    fn hostile_input_is_refused() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "01x",
            "+1",
            "1.",
            "\"\\x\"",
            "\"\\ud800\"",
            "nul",
            "[1] 2",
            "\"a\u{1}\"",
            "1e999",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        let deep = "[".repeat(10_000);
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn arguments_become_text() {
        assert_eq!(Value::Num(3.0).as_argument().as_deref(), Some("3"));
        assert_eq!(Value::Num(0.25).as_argument().as_deref(), Some("0.25"));
        assert_eq!(
            Value::Arr(vec![Value::Num(0.0), Value::Num(2.0)]).as_argument().as_deref(),
            Some("0,2")
        );
        assert_eq!(Value::Bool(true).as_argument().as_deref(), Some("true"));
        assert!(Value::Null.as_argument().is_none());
    }
}
