//! A small JSON reader for what `xcresulttool` prints: objects, arrays, strings with escapes,
//! numbers, booleans and null. Malformed input is `None`, never a panic.

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

static NULL: Json = Json::Null;

impl Json {
    /// The member `key` of an object; `Null` for anything missing, so lookups chain.
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(members) => members.iter().find(|(k, _)| k == key).map_or(&NULL, |(_, v)| v),
            _ => &NULL,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn items(&self) -> &[Json] {
        match self {
            Json::Arr(items) => items,
            _ => &[],
        }
    }
    pub fn members(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(members) => members,
            _ => &[],
        }
    }
}

pub fn parse(text: &str) -> Option<Json> {
    let mut p = Parser { bytes: text.as_bytes(), at: 0 };
    let value = p.value()?;
    p.space();
    (p.at == p.bytes.len()).then_some(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        self.space();
        let hit = self.bytes.get(self.at) == Some(&b);
        if hit {
            self.at += 1;
        }
        hit
    }

    fn word(&mut self, w: &str, v: Json) -> Option<Json> {
        self.bytes[self.at..].starts_with(w.as_bytes()).then(|| {
            self.at += w.len();
            v
        })
    }

    fn value(&mut self) -> Option<Json> {
        self.space();
        match self.bytes.get(self.at)? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Json::Str),
            b't' => self.word("true", Json::Bool(true)),
            b'f' => self.word("false", Json::Bool(false)),
            b'n' => self.word("null", Json::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Option<Json> {
        self.at += 1;
        let mut members = Vec::new();
        if self.eat(b'}') {
            return Some(Json::Obj(members));
        }
        loop {
            self.space();
            let key = self.string()?;
            if !self.eat(b':') {
                return None;
            }
            members.push((key, self.value()?));
            if self.eat(b'}') {
                return Some(Json::Obj(members));
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.at += 1;
        let mut items = Vec::new();
        if self.eat(b']') {
            return Some(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            if self.eat(b']') {
                return Some(Json::Arr(items));
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(|b| b.is_ascii_digit() || b"+-.eE".contains(b)) {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at]).ok()?.parse().ok().map(Json::Num)
    }

    fn string(&mut self) -> Option<String> {
        if self.bytes.get(self.at) != Some(&b'"') {
            return None;
        }
        self.at += 1;
        let mut out = Vec::new();
        loop {
            match *self.bytes.get(self.at)? {
                b'"' => break,
                b'\\' => {
                    self.at += 1;
                    self.escape(&mut out)?;
                }
                b => out.push(b),
            }
            self.at += 1;
        }
        self.at += 1;
        String::from_utf8(out).ok()
    }

    /// One escape after the backslash; leaves `at` on its last byte.
    fn escape(&mut self, out: &mut Vec<u8>) -> Option<()> {
        let simple = match *self.bytes.get(self.at)? {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'b' => 8,
            b'f' => 12,
            b'u' => return self.unicode(out),
            other => other, // \" \\ \/
        };
        out.push(simple);
        Some(())
    }

    fn unicode(&mut self, out: &mut Vec<u8>) -> Option<()> {
        let mut code = self.hex4()?;
        if (0xD800..0xDC00).contains(&code) && self.bytes.get(self.at + 1..self.at + 3) == Some(b"\\u") {
            self.at += 2;
            let low = self.hex4()?;
            code = 0x10000 + ((code - 0xD800) << 10) + (low.checked_sub(0xDC00)?);
        }
        let c = char::from_u32(code).unwrap_or('\u{FFFD}');
        out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
        Some(())
    }

    /// The four hex digits after `u`; leaves `at` on the last one.
    fn hex4(&mut self) -> Option<u32> {
        let digits = std::str::from_utf8(self.bytes.get(self.at + 1..self.at + 5)?).ok()?;
        self.at += 4;
        u32::from_str_radix(digits, 16).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A7.9
    #[test]
    fn reads_what_xcresulttool_prints_and_refuses_broken_input() {
        let v = parse(r#" { "a" : [1, -2.5e1, true, null], "s": "q\"\\\/\né😀", "o": {} } "#).unwrap();
        assert_eq!(v.get("a").items(), [Json::Num(1.0), Json::Num(-25.0), Json::Bool(true), Json::Null]);
        assert_eq!(v.get("s").str(), Some("q\"\\/\né😀"));
        assert_eq!(v.get("o"), &Json::Obj(vec![]));
        assert_eq!(v.get("missing").get("deeper"), &Json::Null);
        assert_eq!(parse(r#"{"k":"中文 path"}"#).unwrap().get("k").str(), Some("中文 path"));
        for bad in ["", "{", "[1,]", r#"{"a" 1}"#, r#""open"#, "tru", "{} x", r#""\u12""#] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
