//! JSON parse + serialize, zero-dependency. Carried from the v1/v2
//! orchestrator parser (same grammar coverage) and extended with a
//! serializer and a canonical form (keys sorted, no insignificant
//! whitespace) for signed bodies (API-V1 section 4).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }
    pub fn s(v: impl AsRef<str>) -> Json { Json::Str(v.as_ref().to_string()) }
    pub fn n(v: f64) -> Json {
        Json::Num(v)
    }
    pub fn b(v: bool) -> Json {
        Json::Bool(v)
    }

    pub fn to_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, false);
        out
    }

    /// Canonical form for signing: object keys sorted recursively, no
    /// insignificant whitespace, numbers in shortest round-trip form.
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, true);
        out
    }

    fn write(&self, out: &mut String, canonical: bool) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{}", n));
                }
            }
            Json::Str(s) => {
                out.push_str(&escape(s));
            }
            Json::Arr(items) => {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    it.write(out, canonical);
                }
                out.push(']');
            }
            Json::Obj(pairs) => {
                out.push('{');
                if canonical {
                    let mut sorted: Vec<&(String, Json)> = pairs.iter().collect();
                    sorted.sort_by(|a, b| a.0.cmp(&b.0));
                    // dedup by key, last wins (matches get())
                    let mut dedup: BTreeMap<&str, &Json> = BTreeMap::new();
                    for (k, v) in &sorted {
                        dedup.insert(k.as_str(), v);
                    }
                    let mut first = true;
                    for (k, v) in dedup {
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        out.push_str(&escape(k));
                        out.push(':');
                        v.write(out, canonical);
                    }
                } else {
                    for (i, (k, v)) in pairs.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&escape(k));
                        out.push(':');
                        v.write(out, canonical);
                    }
                }
                out.push('}');
            }
        }
    }
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser { bytes: text.as_bytes(), pos: 0 };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(format!("trailing input at {}", p.pos));
    }
    Ok(v)
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }
    fn peek(&self) -> Result<u8, String> {
        self.bytes.get(self.pos).copied().ok_or_else(|| "unexpected end of input".to_string())
    }
    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek()? {
            b'{' => {
                self.pos += 1;
                let mut pairs = Vec::new();
                self.skip_ws();
                if self.peek()? == b'}' {
                    self.pos += 1;
                    return Ok(Json::Obj(pairs));
                }
                loop {
                    self.skip_ws();
                    let key = match self.value()? {
                        Json::Str(s) => s,
                        _ => return Err("object key must be a string".into()),
                    };
                    self.skip_ws();
                    if self.peek()? != b':' {
                        return Err("expected ':' in object".into());
                    }
                    self.pos += 1;
                    let val = self.value()?;
                    pairs.push((key, val));
                    self.skip_ws();
                    match self.peek()? {
                        b',' => self.pos += 1,
                        b'}' => {
                            self.pos += 1;
                            return Ok(Json::Obj(pairs));
                        }
                        c => return Err(format!("expected ',' or '}}' in object, got '{}'", c as char)),
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek()? == b']' {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.skip_ws();
                    match self.peek()? {
                        b',' => self.pos += 1,
                        b']' => {
                            self.pos += 1;
                            return Ok(Json::Arr(items));
                        }
                        c => return Err(format!("expected ',' or ']' in array, got '{}'", c as char)),
                    }
                }
            }
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => {
                self.lit("true")?;
                Ok(Json::Bool(true))
            }
            b'f' => {
                self.lit("false")?;
                Ok(Json::Bool(false))
            }
            b'n' => {
                self.lit("null")?;
                Ok(Json::Null)
            }
            _ => self.number(),
        }
    }
    fn lit(&mut self, word: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(format!("invalid literal at {}", self.pos))
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.peek()?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
                                .map_err(|_| "bad \\u escape")?;
                            let code = u32::from_str_radix(hex, 16).map_err(|_| "bad \\u escape")?;
                            self.pos += 4;
                            if (0xD800..0xDC00).contains(&code) && self.bytes[self.pos..].starts_with(b"\\u") {
                                let hex2 = std::str::from_utf8(&self.bytes[self.pos + 2..self.pos + 6])
                                    .map_err(|_| "bad \\u escape")?;
                                if let Ok(lo) = u32::from_str_radix(hex2, 16) {
                                    if (0xDC00..0xE000).contains(&lo) {
                                        self.pos += 6;
                                        let cc = 0x10000 + ((code - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(cc).unwrap_or('\u{FFFD}'));
                                        continue;
                                    }
                                }
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err("bad escape".into()),
                    }
                }
                _ => {
                    let start = self.pos - 1;
                    let len = if c < 0x80 {
                        1
                    } else if c >> 5 == 0b110 {
                        2
                    } else if c >> 4 == 0b1110 {
                        3
                    } else {
                        4
                    };
                    let end = start + len;
                    let s = std::str::from_utf8(&self.bytes[start..end]).map_err(|_| "bad utf-8")?;
                    out.push_str(s);
                    self.pos = end;
                }
            }
        }
    }
    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
        {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| "bad number")?;
        text.parse::<f64>().map(Json::Num).map_err(|_| format!("bad number '{}'", text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_canonical() {
        let v = parse(r#"{"b":1,"a":[true,null,"x\n"],"n":2.5}"#).unwrap();
        assert_eq!(v.get("n").unwrap().as_num(), Some(2.5));
        assert_eq!(v.canonical(), r#"{"a":[true,null,"x\n"],"b":1,"n":2.5}"#);
        assert_eq!(parse(&v.to_string()).unwrap(), v);
    }
}

/// Pretty-printer (2-space indent) for CLI --json output.
pub fn pretty(j: &Json, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let pad2 = "  ".repeat(indent + 1);
    match j {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Num(n) => if n.fract() == 0.0 && n.abs() < 9e15 { format!("{}", *n as i64) } else { format!("{}", n) },
        Json::Str(s) => escape(s),
        Json::Arr(items) => {
            if items.is_empty() {
                return "[]".into();
            }
            let inner: Vec<String> = items.iter().map(|i| format!("{}{}", pad2, pretty(i, indent + 1))).collect();
            format!("[\n{}\n{}]", inner.join(",\n"), pad)
        }
        Json::Obj(pairs) => {
            if pairs.is_empty() {
                return "{}".into();
            }
            let inner: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("{}{}: {}", pad2, escape(k), pretty(v, indent + 1)))
                .collect();
            format!("{{\n{}\n{}}}", inner.join(",\n"), pad)
        }
    }
}
