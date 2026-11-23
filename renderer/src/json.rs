//! A small JSON reader: enough for looks.js and the feed's replies, without pulling in a dependency.
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Json { Null, Bool(bool), Num(f64), Str(String), Arr(Vec<Json>), Obj(Vec<(String, Json)>) }

static NULL: Json = Json::Null;

impl Json {
    pub fn parse(s: &str) -> Result<Json, String> {
        let mut p = P { b: s.as_bytes(), i: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.b.len() { return Err(format!("trailing text at {}", p.i)); }
        Ok(v)
    }
    /// a member of an object (Null when missing or not an object; the last one if a name repeats, as Python and JS read it)
    pub fn get(&self, k: &str) -> &Json {
        match self { Json::Obj(m) => m.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v).unwrap_or(&NULL), _ => &NULL }
    }
    pub fn entries(&self) -> &[(String, Json)] { match self { Json::Obj(m) => m, _ => &[] } }
    pub fn items(&self) -> &[Json] { match self { Json::Arr(a) => a, _ => &[] } }
    pub fn str(&self) -> Option<&str> { match self { Json::Str(s) => Some(s), _ => None } }
    pub fn num(&self) -> Option<f64> { match self { Json::Num(n) => Some(*n), _ => None } }
    pub fn truthy(&self) -> bool {
        match self { Json::Null => false, Json::Bool(b) => *b, Json::Num(n) => *n != 0.0, Json::Str(s) => !s.is_empty(), _ => true }
    }
    pub fn is_null(&self) -> bool { matches!(self, Json::Null) }
    /// an object's members as a map, for lookups that don't care about order
    pub fn map(&self) -> BTreeMap<String, Json> { self.entries().iter().cloned().collect() }
}

struct P<'a> { b: &'a [u8], i: usize }

impl P<'_> {
    fn ws(&mut self) { while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') { self.i += 1; } }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.b.get(self.i) == Some(&c) { self.i += 1; Ok(()) } else { Err(format!("expected '{}' at {}", c as char, self.i)) }
    }
    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') { self.i += 1; return Ok(Json::Obj(m)); }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.eat(b':')?;
                    m.push((k, self.value()?));
                    self.ws();
                    match self.b.get(self.i) { Some(b',') => self.i += 1, Some(b'}') => { self.i += 1; return Ok(Json::Obj(m)); }
                                               _ => return Err(format!("bad object at {}", self.i)) }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') { self.i += 1; return Ok(Json::Arr(a)); }
                loop {
                    a.push(self.value()?);
                    self.ws();
                    match self.b.get(self.i) { Some(b',') => self.i += 1, Some(b']') => { self.i += 1; return Ok(Json::Arr(a)); }
                                               _ => return Err(format!("bad array at {}", self.i)) }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') if self.b[self.i..].starts_with(b"true") => { self.i += 4; Ok(Json::Bool(true)) }
            Some(b'f') if self.b[self.i..].starts_with(b"false") => { self.i += 5; Ok(Json::Bool(false)) }
            Some(b'n') if self.b[self.i..].starts_with(b"null") => { self.i += 4; Ok(Json::Null) }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let s = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') { self.i += 1; }
                std::str::from_utf8(&self.b[s..self.i]).ok().and_then(|t| t.parse().ok()).map(Json::Num)
                    .ok_or_else(|| format!("bad number at {s}"))
            }
            _ => Err(format!("unexpected text at {}", self.i)),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        if self.b.get(self.i) != Some(&b'"') { return Err(format!("expected a string at {}", self.i)); }
        self.i += 1;
        let mut out: Vec<u16> = Vec::new();
        let mut raw = Vec::new();   // plain UTF-8 bytes waiting to be added
        loop {
            let c = *self.b.get(self.i).ok_or("unterminated string")?;
            self.i += 1;
            match c {
                b'"' => { flush(&mut raw, &mut out); return Ok(String::from_utf16_lossy(&out)); }
                b'\\' => {
                    flush(&mut raw, &mut out);
                    let e = *self.b.get(self.i).ok_or("unterminated escape")?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(10), b't' => out.push(9), b'r' => out.push(13), b'b' => out.push(8), b'f' => out.push(12),
                        b'u' => {
                            let h = std::str::from_utf8(self.b.get(self.i..self.i + 4).ok_or("short \\u")?).map_err(|e| e.to_string())?;
                            out.push(u16::from_str_radix(h, 16).map_err(|e| e.to_string())?);   // surrogate pairs come as two
                            self.i += 4;
                        }
                        other => out.push(other as u16),
                    }
                }
                _ => raw.push(c),
            }
        }
    }
}

fn flush(raw: &mut Vec<u8>, out: &mut Vec<u16>) {
    if !raw.is_empty() { out.extend(String::from_utf8_lossy(raw).encode_utf16()); raw.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        let j = Json::parse(r#"{"a": [1, -2.5e1, true, null], "b": "x\"yé\n", "c": {}}"#).unwrap();
        assert_eq!(j.get("a").items()[1].num(), Some(-25.0));
        assert_eq!(j.get("b").str(), Some("x\"yé\n"));
        assert!(j.get("missing").is_null());
        assert_eq!(Json::parse(r#""😀""#).unwrap().str(), Some("😀"));
        assert!(Json::parse("[1,]").is_err());
    }
}
