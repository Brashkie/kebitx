//! Minimal, std-only JSON parser. Tolerates Bedrock-style comments (`//` and `/* */`),
//! which vanilla files use and a strict JSON parser would reject.

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(JsonMap),
}

/// A JSON object: entries sorted by key, keys unique.
///
/// Behaves like the `BTreeMap` it replaces — iteration in key order, a repeated
/// key keeps its LAST value, lookup by binary search — but it is one vector,
/// allocated once per object. Model files hold hundreds of thousands of tiny
/// objects (`{"uv": [..], "uv_size": [..]}`), and a `BTreeMap` allocates a
/// whole node for each: measured, that was most of the parse time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JsonMap(Vec<(String, Json)>);

type Entry<'a> = (&'a String, &'a Json);
type EntryIter<'a> =
    std::iter::Map<std::slice::Iter<'a, (String, Json)>, fn(&'a (String, Json)) -> Entry<'a>>;

impl JsonMap {
    /// From entries in file order: sorted by key (stable), and of repeated
    /// keys the last one written wins, as in a map that is inserted into.
    pub fn from_entries(mut v: Vec<(String, Json)>) -> Self {
        if v.windows(2).any(|w| w[0].0 >= w[1].0) {
            v.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out: Vec<(String, Json)> = Vec::with_capacity(v.len());
            for e in v {
                match out.last_mut() {
                    Some(last) if last.0 == e.0 => *last = e,
                    _ => out.push(e),
                }
            }
            v = out;
        }
        JsonMap(v)
    }
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0
            .binary_search_by(|(k, _)| k.as_str().cmp(key))
            .ok()
            .map(|i| &self.0[i].1)
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> EntryIter<'_> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl Iterator<Item = &Json> {
        self.0.iter().map(|(_, v)| v)
    }
}

/// Serialize compactly. Strings are escaped as JSON requires; whole numbers
/// print without a fraction (`1`, not `1.0`), as the game's own files do.
pub fn to_string(v: &Json) -> String {
    let mut out = String::new();
    write_value(v, &mut out);
    out
}

fn write_value(v: &Json, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => {
            out.push_str(&format!("{}", *n as i64))
        }
        Json::Num(n) if n.is_finite() => out.push_str(&format!("{n}")),
        Json::Num(_) => out.push_str("null"), // JSON has no NaN or infinity
        Json::Str(s) => write_str(s, out),
        Json::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(x, out);
            }
            out.push(']');
        }
        Json::Obj(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write_value(x, out);
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
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

impl<'a> IntoIterator for &'a JsonMap {
    type Item = Entry<'a>;
    type IntoIter = EntryIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug)]
pub struct JsonError {
    pub msg: String,
    pub pos: usize,
}

/// Maximum nesting of objects and arrays. The parser recurses once per level, so
/// without a bound a file of nothing but `[[[[...` overflows the stack and kills
/// the process. Real Bedrock files stay far below this (vanilla tops out around
/// a dozen levels); anything deeper is reported as unreadable, not followed.
pub const MAX_DEPTH: usize = 128;

pub fn parse(input: &str) -> Result<Json, JsonError> {
    let mut p = Parser {
        src: input,
        b: input.as_bytes(),
        i: 0,
        depth: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(p.err("trailing content after root value"));
    }
    Ok(v)
}

impl Json {
    pub fn as_object(&self) -> Option<&JsonMap> {
        if let Json::Obj(m) = self {
            Some(m)
        } else {
            None
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Json>> {
        if let Json::Arr(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Json::Str(s) = self {
            Some(s.as_str())
        } else {
            None
        }
    }
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.as_object().and_then(|m| m.get(key))
    }
}

struct Parser<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, m: &str) -> JsonError {
        JsonError {
            msg: m.to_string(),
            pos: self.i,
        }
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn ws(&mut self) {
        loop {
            while self.i < self.b.len() && (self.b[self.i] as char).is_ascii_whitespace() {
                self.i += 1;
            }
            if self.i + 1 < self.b.len() && self.b[self.i] == b'/' && self.b[self.i + 1] == b'/' {
                self.i += 2;
                while self.i < self.b.len() && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
                continue;
            }
            if self.i + 1 < self.b.len() && self.b[self.i] == b'/' && self.b[self.i + 1] == b'*' {
                self.i += 2;
                while self.i + 1 < self.b.len()
                    && !(self.b[self.i] == b'*' && self.b[self.i + 1] == b'/')
                {
                    self.i += 1;
                }
                // An unterminated comment must not move the cursor past the end.
                self.i = (self.i + 2).min(self.b.len());
                continue;
            }
            break;
        }
    }
    fn value(&mut self) -> Result<Json, JsonError> {
        self.ws();
        match self.peek() {
            Some(b'{') | Some(b'[') => {
                if self.depth >= MAX_DEPTH {
                    return Err(self.err("nesting deeper than MAX_DEPTH"));
                }
                self.depth += 1;
                let v = if self.peek() == Some(b'{') {
                    self.object()
                } else {
                    self.array()
                };
                self.depth -= 1;
                v
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') | Some(b'f') => self.boolean(),
            Some(b'n') => self.null(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(self.err("unexpected JSON value")),
        }
    }
    fn object(&mut self) -> Result<Json, JsonError> {
        self.i += 1;
        let mut map: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(JsonMap::default()));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected string key"));
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.i += 1;
            let val = self.value()?;
            map.push((key, val));
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    // Trailing comma: `{"a": 1,}`. Not valid JSON, but real packs
                    // ship it, and Minecraft accepts it. Rejecting it made a whole
                    // atlas fail to parse and get skipped without a word.
                    self.ws();
                    if self.peek() == Some(b'}') {
                        self.i += 1;
                        break;
                    }
                    continue;
                }
                Some(b'}') => {
                    self.i += 1;
                    break;
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
        Ok(Json::Obj(JsonMap::from_entries(map)))
    }
    fn array(&mut self) -> Result<Json, JsonError> {
        self.i += 1;
        let mut arr = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(arr));
        }
        loop {
            let val = self.value()?;
            arr.push(val);
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    self.ws(); // trailing comma: `[1, 2,]`, same reasoning as objects
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        break;
                    }
                    continue;
                }
                Some(b']') => {
                    self.i += 1;
                    break;
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
        Ok(Json::Arr(arr))
    }
    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1;
        let mut s = String::new();
        loop {
            // Fast path: copy the whole run up to the next quote or backslash in
            // one go. Both are ASCII, so the run ends on a char boundary and the
            // slice of the original `&str` is valid UTF-8 as it stands.
            let start = self.i;
            while self.i < self.b.len() && self.b[self.i] != b'"' && self.b[self.i] != b'\\' {
                self.i += 1;
            }
            s.push_str(&self.src[start..self.i]);
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    break;
                }
                Some(_) => {
                    self.i += 1;
                    match self.peek() {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'r') => s.push('\r'),
                        Some(b'b') => s.push('\u{0008}'),
                        Some(b'f') => s.push('\u{000C}'),
                        Some(b'u') => {
                            let mut code: u32 = 0;
                            for _ in 0..4 {
                                self.i += 1;
                                let c = self.peek().ok_or_else(|| self.err("incomplete \\u"))?;
                                let d = (c as char)
                                    .to_digit(16)
                                    .ok_or_else(|| self.err("invalid hex in \\u"))?;
                                code = code * 16 + d;
                            }
                            if let Some(ch) = char::from_u32(code) {
                                s.push(ch);
                            }
                        }
                        _ => return Err(self.err("invalid escape")),
                    }
                    self.i += 1;
                }
            }
        }
        Ok(s)
    }
    fn boolean(&mut self) -> Result<Json, JsonError> {
        if self.b[self.i..].starts_with(b"true") {
            self.i += 4;
            Ok(Json::Bool(true))
        } else if self.b[self.i..].starts_with(b"false") {
            self.i += 5;
            Ok(Json::Bool(false))
        } else {
            Err(self.err("invalid boolean literal"))
        }
    }
    fn null(&mut self) -> Result<Json, JsonError> {
        if self.b[self.i..].starts_with(b"null") {
            self.i += 4;
            Ok(Json::Null)
        } else {
            Err(self.err("invalid null literal"))
        }
    }
    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-' {
                self.i += 1;
            } else {
                break;
            }
        }
        let slice = &self.src[start..self.i];
        if let Some(n) = fast_number(slice.as_bytes()) {
            return Ok(Json::Num(n));
        }
        slice
            .parse::<f64>()
            .map(Json::Num)
            .map_err(|_| self.err("invalid number"))
    }
}

/// Exact fast path for the numbers model files are made of (`12`, `-0.5`,
/// `3.125`): at most 15 significant digits, no exponent. The mantissa fits in
/// an f64 exactly (< 2^53) and so does 10^k (k ≤ 15), and IEEE division is
/// correctly rounded, so `mantissa / 10^k` is the same f64 the full parser
/// gives (Clinger's fast path). Anything else returns `None` and goes the
/// slow, general way.
fn fast_number(b: &[u8]) -> Option<f64> {
    const POW10: [f64; 16] = [
        1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
    ];
    let (neg, digits) = match b.first()? {
        b'-' => (true, &b[1..]),
        _ => (false, b),
    };
    let mut mantissa: u64 = 0;
    let mut count = 0usize; // digits accumulated
    let mut frac = 0usize; // digits after the point
    let mut seen_point = false;
    let mut seen_digit = false;
    for &c in digits {
        match c {
            b'0'..=b'9' => {
                seen_digit = true;
                // leading zeros cost nothing; they do not count against the limit
                if mantissa != 0 || c != b'0' {
                    count += 1;
                }
                if count > 15 {
                    return None;
                }
                mantissa = mantissa * 10 + (c - b'0') as u64;
                if seen_point {
                    frac += 1;
                }
            }
            b'.' if !seen_point => seen_point = true,
            _ => return None, // exponent, second point, sign in the middle
        }
    }
    if !seen_digit || frac > 15 || digits.last() == Some(&b'.') || digits.first() == Some(&b'.') {
        return None;
    }
    let v = mantissa as f64 / POW10[frac];
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skips_comments_and_reads_nested() {
        let src = r#"{ // line
            "a": { "b": "c" }, /* block */ "n": 3 }"#;
        let j = parse(src).unwrap();
        assert_eq!(
            j.get("a").and_then(|x| x.get("b")).and_then(Json::as_str),
            Some("c")
        );
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(&ok).is_ok());
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(parse(&deep).is_err());
        // far past the limit: must return, not overflow the stack
        assert!(parse(&"[{\"a\":".repeat(1_000_000)).is_err());
    }

    #[test]
    fn fast_numbers_equal_the_standard_parser_bit_for_bit() {
        let mut cases: Vec<String> = [
            "0",
            "-0",
            "1",
            "12",
            "-3",
            "0.5",
            "-0.5",
            "3.125",
            "12.5",
            "0.1",
            "0.2",
            "0.3",
            "123456789012345",
            "1.23456789012345",
            "0.000000000000001",
            "-62.0625",
            "00012.50",
            "9007199254740991",
            "1e5",
            "1.5E-3",
            "-",
            ".5",
            "5.",
            "1.2.3",
            "1-2",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        // a deterministic sweep of decimals like the ones in model files
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let int = x % 100_000;
            let frac = (x >> 20) % 10_000_000;
            let places = 1 + (x >> 50) as usize % 7;
            let sign = if x & 1 == 0 { "" } else { "-" };
            cases.push(
                format!("{sign}{int}.{frac:0>7}")
                    .chars()
                    .take(sign.len() + int.to_string().len() + 1 + places)
                    .collect(),
            );
        }
        for s in &cases {
            let fast = fast_number(s.as_bytes());
            match s.parse::<f64>() {
                Ok(std) => {
                    if let Some(f) = fast {
                        assert_eq!(f.to_bits(), std.to_bits(), "{s}");
                    }
                }
                Err(_) => assert_eq!(fast, None, "{s} is not a number"),
            }
        }
        // the common forms do take the fast path
        for s in ["12", "-0.5", "3.125", "123456789012345"] {
            assert!(fast_number(s.as_bytes()).is_some(), "{s}");
        }
    }

    #[test]
    fn writing_and_reading_back_gives_the_same_value() {
        let src = r#"{"b": [1, -2.5, 3e2, true, null], "a": "q\"uo\\te\n\u0001 ñ", "c": {}}"#;
        let v = parse(src).unwrap();
        let written = to_string(&v);
        assert_eq!(parse(&written).unwrap(), v);
        assert!(written.contains("[1,-2.5,300,true,null]"), "{written}");
    }

    #[test]
    fn objects_behave_like_the_map_they_replace() {
        let j = parse(r#"{"b": 1, "a": 2, "c": 3, "a": 4}"#).unwrap();
        let m = j.as_object().unwrap();
        assert_eq!(
            m.keys().map(String::as_str).collect::<Vec<_>>(),
            ["a", "b", "c"],
            "key order"
        );
        assert_eq!(
            m.get("a"),
            Some(&Json::Num(4.0)),
            "a repeated key keeps the last value"
        );
        assert_eq!(m.len(), 3);
        assert!(m.get("zz").is_none() && m.get("").is_none());
        // same content in another order is the same object
        assert_eq!(
            parse(r#"{"x": [1], "y": {}}"#).unwrap(),
            parse(r#"{"y": {}, "x": [1]}"#).unwrap()
        );
    }

    #[test]
    fn strings_keep_escapes_and_non_ascii_exact() {
        let j = parse(r#"{"k": "a\"b ñandú é\\ \n/ 日本 end", "": ""}"#).unwrap();
        assert_eq!(
            j.get("k").and_then(Json::as_str),
            Some("a\"b ñandú é\\ \n/ 日本 end")
        );
        assert_eq!(j.get("").and_then(Json::as_str), Some(""));
    }

    #[test]
    fn malformed_input_never_panics() {
        for src in [
            "/*", "[1, /* x", "\"\\", "\"\\u12", "tru", "nul", "-", "{\"a\"", "[", "{", "",
        ] {
            let _ = parse(src);
        }
    }
}
