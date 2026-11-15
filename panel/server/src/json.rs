//! Minimalny parser JSON (bez zależności) – tylko do odczytu odpowiedzi mostu i Jellyfin w serwerze panelu
//! (odtwarzanie pozycji bez legacy, `vod`). Liczby jako f64, obiekty jako lista par (kolejność zachowana).

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// Pole obiektu (pierwsze o tej nazwie).
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn arr(&self) -> &[Value] {
        match self {
            Value::Arr(a) => a,
            _ => &[],
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 64;

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("JSON: nadmiarowe znaki od {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("JSON: {what} (znak {})", self.i))
    }
    fn lit(&mut self, s: &str, v: Value) -> Result<Value, String> {
        if self.b[self.i..].starts_with(s.as_bytes()) {
            self.i += s.len();
            Ok(v)
        } else {
            self.err("nieznana wartość")
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return self.err("za głęboko zagnieżdżony");
        }
        self.ws();
        match self.b.get(self.i) {
            None => self.err("koniec danych"),
            Some(b'{') => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(m));
                }
                loop {
                    self.ws();
                    if self.b.get(self.i) != Some(&b'"') {
                        return self.err("oczekiwano klucza");
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return self.err("oczekiwano „:”");
                    }
                    self.i += 1;
                    m.push((k, self.value(depth + 1)?));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(m));
                        }
                        _ => return self.err("oczekiwano „,” albo „}”"),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Arr(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(a));
                        }
                        _ => return self.err("oczekiwano „,” albo „]”"),
                    }
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'n') => self.lit("null", Value::Null),
            Some(_) => {
                let start = self.i;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[start..self.i])
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(Value::Num)
                    .map_or_else(|| self.err("zła liczba"), Ok)
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let h = self
            .b
            .get(self.i..self.i + 4)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u32::from_str_radix(h, 16).ok());
        match h {
            Some(n) => {
                self.i += 4;
                Ok(n)
            }
            None => self.err("zły \\u"),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // cudzysłów otwierający
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.b.len() && self.b[self.i] != b'"' && self.b[self.i] != b'\\' {
                self.i += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.b[start..self.i])
                    .map_err(|_| "JSON: zły UTF-8".to_string())?,
            );
            match self.b.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let c = self.b.get(self.i).copied();
                    self.i += 1;
                    match c {
                        Some(b'"') => out.push('"'),
                        Some(b'\\') => out.push('\\'),
                        Some(b'/') => out.push('/'),
                        Some(b'b') => out.push('\u{8}'),
                        Some(b'f') => out.push('\u{c}'),
                        Some(b'n') => out.push('\n'),
                        Some(b'r') => out.push('\r'),
                        Some(b't') => out.push('\t'),
                        Some(b'u') => {
                            let mut n = self.hex4()?;
                            if (0xd800..0xdc00).contains(&n)
                                && self.b.get(self.i..self.i + 2) == Some(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                n = 0x10000
                                    + ((n - 0xd800) << 10)
                                    + (lo.wrapping_sub(0xdc00) & 0x3ff);
                            }
                            out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                        }
                        _ => return self.err("zła sekwencja \\"),
                    }
                }
                _ => return self.err("niezamknięty tekst"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let v = parse(r#" {"ok":true,"result":{"Items":[{"Id":"8cfa","Path":"/media/a b.mp4","N":-1.5e2}],"x":null},"s":"\u0105\"\n\ud83d\ude00"} "#).unwrap();
        let it = &v.get("result").unwrap().get("Items").unwrap().arr()[0];
        assert_eq!(it.get("Id").and_then(Value::str), Some("8cfa"));
        assert_eq!(it.get("Path").and_then(Value::str), Some("/media/a b.mp4"));
        assert_eq!(it.get("N").and_then(Value::num), Some(-150.0));
        assert_eq!(v.get("s").and_then(Value::str), Some("ą\"\n😀"));
        assert_eq!(v.get("ok"), Some(&Value::Bool(true)));
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "tru",
            "\"abc",
            "{} x",
            "\"\\q\"",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(parse(&"[".repeat(100)).is_err());
    }
}
