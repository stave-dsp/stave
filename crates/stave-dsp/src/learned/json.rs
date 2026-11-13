//! The JSON subset model files need (RFC 8259, all of it): a small
//! recursive-descent reader into a tree. Crate-private; model loaders
//! walk the tree and report their own errors.
//!
//! Numbers are read as `f64` with the standard library's correctly
//! rounded parser, so a weight written from an `f32` reads back as the
//! same `f32` after one more rounding. Nesting deeper than
//! [`MAX_DEPTH`] is refused rather than recursing without bound (model
//! files nest four or five levels).

use std::fmt;

/// Deepest nesting accepted. The reader recurses once per level, so the
/// bound is what keeps a hostile file from exhausting the stack.
pub(crate) const MAX_DEPTH: usize = 128;

/// One JSON value. Objects keep their keys in file order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// Where and why a text is not JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JsonError {
    /// Byte offset of the problem.
    pub offset: usize,
    pub reason: &'static str,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.reason, self.offset)
    }
}

impl Json {
    /// Parse a whole document (one value, surrounding whitespace allowed).
    pub(crate) fn parse(text: &str) -> Result<Json, JsonError> {
        let mut r = Reader {
            bytes: text.as_bytes(),
            at: 0,
        };
        r.skip_ws();
        let value = r.value(0)?;
        r.skip_ws();
        if r.at != r.bytes.len() {
            return Err(r.error("trailing characters after the value"));
        }
        Ok(value)
    }

    /// The member `key` of an object (`None` for a missing key or a
    /// value that is not an object).
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The member `key`, treating an explicit `null` as missing.
    pub(crate) fn field(&self, key: &str) -> Option<&Json> {
        self.get(key).filter(|v| !matches!(v, Json::Null))
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn error(&self, reason: &'static str) -> JsonError {
        JsonError {
            offset: self.at,
            reason,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect_word(&mut self, word: &'static str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("unknown literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth >= MAX_DEPTH {
            return Err(self.error("nesting deeper than the reader's limit"));
        }
        match self.peek() {
            None => Err(self.error("unexpected end of input")),
            Some(b'n') => self.expect_word("null", Json::Null),
            Some(b't') => self.expect_word("true", Json::Bool(true)),
            Some(b'f') => self.expect_word("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.at += 1; // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']' in an array")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.at += 1; // {
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a string key in an object"));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.error("expected ':' after an object key"));
            }
            self.at += 1;
            self.skip_ws();
            let value = self.value(depth + 1)?;
            members.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.error("expected ',' or '}' in an object")),
            }
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.error("expected a digit")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err(self.error("expected a digit after '.'"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("expected a digit in the exponent"));
            }
        }
        // The span is ASCII by construction.
        let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("");
        text.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(Json::Number)
            .ok_or(JsonError {
                offset: start,
                reason: "number out of range",
            })
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let d = match self.peek() {
                Some(c @ b'0'..=b'9') => c - b'0',
                Some(c @ b'a'..=b'f') => c - b'a' + 10,
                Some(c @ b'A'..=b'F') => c - b'A' + 10,
                _ => return Err(self.error("expected four hex digits after \\u")),
            };
            v = v * 16 + u32::from(d);
            self.at += 1;
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1; // "
        let mut out = String::new();
        loop {
            // Copy the run up to the next quote or escape in one go.
            let run = self.at;
            while matches!(self.peek(), Some(c) if c != b'"' && c != b'\\' && c >= 0x20) {
                self.at += 1;
            }
            // Runs end on ASCII, so they are whole UTF-8 sequences of
            // the (valid UTF-8) input.
            out.push_str(std::str::from_utf8(&self.bytes[run..self.at]).unwrap_or(""));
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let c = self.peek().ok_or(self.error("unterminated escape"))?;
                    self.at += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                if !self.bytes[self.at..].starts_with(b"\\u") {
                                    return Err(self.error("unpaired surrogate"));
                                }
                                self.at += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(self.error("unpaired surrogate"));
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            out.push(char::from_u32(code).ok_or(self.error("invalid \\u escape"))?);
                        }
                        _ => return Err(self.error("unknown escape")),
                    }
                }
                Some(_) => return Err(self.error("control character in a string")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_kind_of_value() {
        let j = Json::parse(
            r#" {"a": [1, -2.5e-3, 0, true, false, null], "b": {"c": "x\"\u00e9\ud83c\udfb8y"}} "#,
        )
        .unwrap();
        let a = j.get("a").unwrap().as_array().unwrap();
        assert_eq!(a[0], Json::Number(1.0));
        assert_eq!(a[1], Json::Number(-2.5e-3));
        assert_eq!(a[3], Json::Bool(true));
        assert_eq!(a[5], Json::Null);
        assert_eq!(
            j.get("b").unwrap().get("c").unwrap().as_str(),
            Some("x\"é🎸y")
        );
        assert!(j.field("missing").is_none());
    }

    #[test]
    fn f32_weights_round_trip_exactly() {
        for w in [0.1f32, -1.0e-7, f32::MAX, f32::MIN_POSITIVE, 1.0 / 3.0] {
            let text = format!("[{}]", f64::from(w));
            let j = Json::parse(&text).unwrap();
            assert_eq!(j.as_array().unwrap()[0].as_f64().unwrap() as f32, w);
        }
    }

    #[test]
    fn refuses_what_is_not_json() {
        for bad in [
            "",
            "[1,]",
            "{\"a\" 1}",
            "01",
            "1.",
            "-",
            "\"abc",
            "[1] x",
            "NaN",
            "1e999",
            "{1:2}",
            "\"\\ud800\"",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?} parsed");
        }
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert!(Json::parse(&deep).is_err());
    }
}
