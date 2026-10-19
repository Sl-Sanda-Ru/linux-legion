//! Минимальный JSON: ровно столько, сколько нужно для профилей подсветки в
//! формате Lenovo Legion Toolkit (массив объектов с числами, строками и
//! вложенными массивами). Тащить serde ради одного файла незачем.

use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    /// Порядок ключей не важен; при выводе — в порядке вставки через `Obj`.
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.get(key),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    // BOM, который оставляют некоторые редакторы Windows.
    if p.s.starts_with(&[0xEF, 0xBB, 0xBF]) {
        p.i = 3;
    }
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("unexpected data at byte {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.i))
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.ws();
        match self.s.get(self.i) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(format!("unexpected token at byte {}", self.i)),
        }
    }

    fn word(&mut self, w: &str, v: Value) -> Result<Value, String> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            Err(format!("unexpected token at byte {}", self.i))
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(c)) {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()
            .and_then(|t| t.parse().ok())
            .map(Value::Number)
            .ok_or_else(|| format!("bad number at byte {start}"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.s.get(self.i).is_some_and(|&c| c != b'"' && c != b'\\') {
                self.i += 1;
            }
            out.push_str(std::str::from_utf8(&self.s[start..self.i]).map_err(|_| "invalid UTF-8".to_string())?);
            match self.s.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    let e = *self.s.get(self.i + 1).ok_or("unterminated string")?;
                    self.i += 2;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let hex = self.s.get(self.i..self.i + 4).ok_or("bad \\u escape")?;
                            let code = u32::from_str_radix(std::str::from_utf8(hex).unwrap_or(""), 16)
                                .map_err(|_| "bad \\u escape".to_string())?;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            self.i += 4;
                        }
                        _ => return Err(format!("bad escape at byte {}", self.i)),
                    }
                }
                _ => return Err("unterminated string".into()),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.eat(b'[')?;
        let mut out = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Value::Array(out));
        }
        loop {
            out.push(self.value()?);
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Array(out));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.i)),
            }
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.eat(b'{')?;
        let mut out = BTreeMap::new();
        self.ws();
        if self.s.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Value::Object(out));
        }
        loop {
            self.ws();
            let k = self.string()?;
            self.eat(b':')?;
            out.insert(k, self.value()?);
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Object(out));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.i)),
            }
        }
    }
}

/// Объект с ключами в заданном порядке (для читаемого вывода).
pub struct Obj(pub Vec<(&'static str, Out)>);

/// Значение для вывода.
pub enum Out {
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<Out>),
    Obj(Obj),
}

/// Вывод с отступами в два пробела — как у Newtonsoft `Formatting.Indented`.
pub fn render(v: &Out) -> String {
    let mut s = String::new();
    write_out(&mut s, v, 0);
    s
}

fn write_out(s: &mut String, v: &Out, depth: usize) {
    let pad = |s: &mut String, d: usize| s.push_str(&"  ".repeat(d));
    match v {
        Out::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        Out::Int(n) => {
            let _ = write!(s, "{n}");
        }
        Out::Str(t) => write_str(s, t),
        Out::List(items) if items.is_empty() => s.push_str("[]"),
        Out::List(items) => {
            s.push_str("[\n");
            for (i, it) in items.iter().enumerate() {
                pad(s, depth + 1);
                write_out(s, it, depth + 1);
                s.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            pad(s, depth);
            s.push(']');
        }
        Out::Obj(Obj(fields)) => {
            s.push_str("{\n");
            for (i, (k, it)) in fields.iter().enumerate() {
                pad(s, depth + 1);
                write_str(s, k);
                s.push_str(": ");
                write_out(s, it, depth + 1);
                s.push_str(if i + 1 < fields.len() { ",\n" } else { "\n" });
            }
            pad(s, depth);
            s.push('}');
        }
    }
}

fn write_str(s: &mut String, t: &str) {
    s.push('"');
    for c in t.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\t' => s.push_str("\\t"),
            '\r' => s.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                let _ = write!(s, "\\u{:04x}", c as u32);
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested() {
        let v = parse("\u{feff}[ {\"a\": [1, -2.5e1, true, null], \"b\": \"x\\\"\\u0041\"} ]").unwrap();
        let o = &v.as_array().unwrap()[0];
        assert_eq!(o.get("b"), Some(&Value::String("x\"A".into())));
        let a = o.get("a").and_then(Value::as_array).unwrap();
        assert_eq!(a[1].as_f64(), Some(-25.0));
        assert_eq!(a[3], Value::Null);
        assert!(parse("[1,]").is_err());
        assert!(parse("{} x").is_err());
    }

    #[test]
    fn render_roundtrip() {
        let out = Out::List(vec![Out::Obj(Obj(vec![
            ("Type", Out::Str("Always".into())),
            ("Keys", Out::List(vec![Out::Int(1), Out::Int(2)])),
            ("Colors", Out::List(vec![])),
            ("On", Out::Bool(false)),
        ]))]);
        let text = render(&out);
        assert!(text.starts_with("[\n  {\n    \"Type\": \"Always\""));
        let v = parse(&text).unwrap();
        assert_eq!(v.as_array().unwrap()[0].get("Keys").and_then(Value::as_array).map(<[_]>::len), Some(2));
    }
}
