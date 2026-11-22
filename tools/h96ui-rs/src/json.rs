//! A small JSON reader for the Xtream API (no dependencies). Lenient where Xtream panels are
//! sloppy: numbers and ids arrive as strings or numbers, fields may be null, objects may be
//! arrays. Accessors therefore coerce (`str()` gives the text of a number too).

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(BTreeMap<String, Value>),
}

static NULL: Value = Value::Null;

impl Value {
    /// Member of an object (Null if absent or not an object).
    pub fn get(&self, k: &str) -> &Value {
        match self {
            Value::Obj(m) => m.get(k).unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    /// Text of a string, number or bool; "" for null/arrays/objects.
    pub fn str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
            Value::Num(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => String::new(),
        }
    }

    /// Number from a number or a numeric string.
    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// Elements of an array, or the values of an object (some panels send {"0":..,"1":..}).
    pub fn items(&self) -> Vec<&Value> {
        match self {
            Value::Arr(v) => v.iter().collect(),
            Value::Obj(m) => m.values().collect(),
            _ => Vec::new(),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { b: text.as_bytes(), i: 0 };
    p.ws();
    if p.b.starts_with(b"\xef\xbb\xbf") {
        p.i = 3;
    }
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("JSON: trailing data at byte {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("JSON: {what} at byte {}", self.i))
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 64 {
            return self.err("nested too deeply");
        }
        self.ws();
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut m = BTreeMap::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(m));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return self.err("expected ':'");
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    m.insert(k, v);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(m));
                        }
                        _ => return self.err("expected ',' or '}'"),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Arr(v));
                }
                loop {
                    v.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(v));
                        }
                        _ => return self.err("expected ',' or ']'"),
                    }
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') if self.b[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Value::Bool(true))
            }
            Some(b'f') if self.b[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Value::Bool(false))
            }
            Some(b'n') if self.b[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Value::Null)
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let st = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.b[st..self.i]).unwrap_or("");
                t.parse().map(Value::Num).or_else(|_| self.err("bad number"))
            }
            _ => self.err("unexpected character"),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.b.get(self.i..self.i + 4).and_then(|h| std::str::from_utf8(h).ok());
        match h.and_then(|h| u32::from_str_radix(h, 16).ok()) {
            Some(v) => {
                self.i += 4;
                Ok(v)
            }
            None => self.err("bad \\u escape"),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.b.get(self.i) != Some(&b'"') {
            return self.err("expected string");
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.b.get(self.i) else { return self.err("unterminated string") };
            self.i += 1;
            match c {
                b'"' => return Ok(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else { return self.err("bad escape") };
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xd800..0xdc00).contains(&cp) && self.b[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000 + ((cp - 0xd800) << 10) + (lo.wrapping_sub(0xdc00) & 0x3ff);
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        x => out.push(x), // \" \\ \/
                    }
                }
                x => out.push(x),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xtream_shapes() {
        let v = parse(r#"{"user_info":{"auth":1,"status":"Active","exp_date":"1767225600","max_connections":"2"},
            "list":[{"stream_id":12,"name":"Café 😀","category_id":"3","x":null},{"stream_id":"13"}],
            "eps":{"1":[{"id":"9","title":"S1E1"}]}, "n":-1.5e2, "t":true}"#)
        .unwrap();
        assert_eq!(v.get("user_info").get("auth").str(), "1");
        assert_eq!(v.get("user_info").get("max_connections").num(), Some(2.0));
        let l = v.get("list").items();
        assert_eq!(l[0].get("stream_id").str(), "12");
        assert_eq!(l[1].get("stream_id").str(), "13");
        assert_eq!(l[0].get("name").str(), "Café 😀");
        assert!(l[0].get("x").is_null() && l[0].get("missing").is_null());
        assert_eq!(v.get("eps").items()[0].items()[0].get("title").str(), "S1E1");
        assert_eq!(v.get("n").num(), Some(-150.0));
        assert!(parse("{\"a\":1} x").is_err());
        assert!(parse("[1,2").is_err());
    }
}
