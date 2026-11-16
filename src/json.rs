pub enum J {
    Null,
    Bool,
    Num,
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

pub fn parse(b: &[u8]) -> Option<J> {
    let mut p = P { b, i: 0, depth: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != b.len() {
        return None;
    }
    Some(v)
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
    depth: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn lit(&mut self, s: &[u8]) -> Option<()> {
        if self.b[self.i..].starts_with(s) {
            self.i += s.len();
            Some(())
        } else {
            None
        }
    }
    fn value(&mut self) -> Option<J> {
        match self.peek()? {
            b'{' => {
                self.depth += 1;
                if self.depth > 10000 {
                    return None;
                }
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.peek()? == b'}' {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        if self.peek()? != b'"' {
                            return None;
                        }
                        let k = self.string()?;
                        self.ws();
                        if self.peek()? != b':' {
                            return None;
                        }
                        self.i += 1;
                        self.ws();
                        let v = self.value()?;
                        m.push((k, v));
                        self.ws();
                        match self.peek()? {
                            b',' => self.i += 1,
                            b'}' => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                self.depth -= 1;
                Some(J::Obj(m))
            }
            b'[' => {
                self.depth += 1;
                if self.depth > 10000 {
                    return None;
                }
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.peek()? == b']' {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        a.push(self.value()?);
                        self.ws();
                        match self.peek()? {
                            b',' => self.i += 1,
                            b']' => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                self.depth -= 1;
                Some(J::Arr(a))
            }
            b'"' => Some(J::Str(self.string()?)),
            b't' => self.lit(b"true").map(|_| J::Bool),
            b'f' => self.lit(b"false").map(|_| J::Bool),
            b'n' => self.lit(b"null").map(|_| J::Null),
            b'-' | b'0'..=b'9' => self.number().map(|_| J::Num),
            _ => None,
        }
    }
    fn digits(&mut self) -> usize {
        let s = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        self.i - s
    }
    fn number(&mut self) -> Option<()> {
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek()? {
            b'0' => self.i += 1,
            b'1'..=b'9' => {
                self.digits();
            }
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        Some(())
    }
    fn hex4(&mut self) -> Option<u32> {
        let h = self.b.get(self.i..self.i + 4)?;
        let s = std::str::from_utf8(h).ok()?;
        let v = u32::from_str_radix(s, 16).ok()?;
        if !h.iter().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        self.i += 4;
        Some(v)
    }
    fn string(&mut self) -> Option<String> {
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek()?;
            match c {
                b'"' => {
                    self.i += 1;
                    break;
                }
                b'\\' => {
                    self.i += 1;
                    let e = self.peek()?;
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
                            let r = self.hex4()?;
                            if (0xD800..0xE000).contains(&r) {
                                // A surrogate: valid only as the first of a pair.
                                let save = self.i;
                                let mut ch = '\u{FFFD}';
                                if r < 0xDC00 && self.b[self.i..].starts_with(b"\\u") {
                                    self.i += 2;
                                    match self.hex4() {
                                        Some(r2) if (0xDC00..0xE000).contains(&r2) => {
                                            ch = char::from_u32(0x10000 + ((r - 0xD800) << 10) + (r2 - 0xDC00))
                                                .unwrap_or('\u{FFFD}');
                                        }
                                        _ => self.i = save,
                                    }
                                }
                                ch
                            } else {
                                char::from_u32(r).unwrap_or('\u{FFFD}')
                            }
                        }
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                0..=0x1f => return None,
                _ => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
        Some(crate::gofs::go_runes(&crate::gofs::text(&out)))
    }
}

pub fn fold_eq(key: &str, field: &str) -> bool {
    let mut k = key.chars().map(|c| match c {
        'ſ' => 's',
        '\u{212A}' => 'k',
        c => c.to_ascii_lowercase(),
    });
    let mut f = field.chars();
    loop {
        match (k.next(), f.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a == b.to_ascii_lowercase() => {}
            _ => return false,
        }
    }
}

pub fn members(v: Option<&J>) -> Option<&[(String, J)]> {
    match v? {
        J::Obj(m) => Some(m),
        J::Null => Some(&[]),
        _ => None,
    }
}

pub fn raw<'a>(m: &'a [(String, J)], field: &str) -> Option<&'a J> {
    m.iter().rev().find(|(k, _)| fold_eq(k, field)).map(|(_, v)| v)
}

pub fn string_field(m: &[(String, J)], field: &str) -> Result<String, ()> {
    let mut out = String::new();
    let mut err = false;
    for (k, v) in m {
        if fold_eq(k, field) {
            match v {
                J::Str(s) => out = s.clone(),
                J::Null => {}
                _ => err = true,
            }
        }
    }
    if err {
        Err(())
    } else {
        Ok(out)
    }
}

pub fn string_field_lenient(m: &[(String, J)], field: &str) -> String {
    let mut out = String::new();
    for (k, v) in m {
        if let (true, J::Str(s)) = (fold_eq(k, field), v) {
            out = s.clone();
        }
    }
    out
}

pub fn as_string(v: Option<&J>) -> Option<String> {
    match v? {
        J::Str(s) => Some(s.clone()),
        J::Null => Some(String::new()),
        _ => None,
    }
}

pub fn as_strings(v: Option<&J>) -> Option<Vec<String>> {
    match v? {
        J::Null => Some(Vec::new()),
        J::Arr(a) => a.iter().map(|x| as_string(Some(x))).collect(),
        _ => None,
    }
}

pub fn ordered(v: Option<&J>) -> &[(String, J)] {
    match v {
        Some(J::Obj(m)) => m,
        _ => &[],
    }
}

pub fn has_members(v: Option<&J>) -> bool {
    matches!(v, Some(J::Obj(m)) if !m.is_empty())
}

pub fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if crate::gofs::raw_byte(c).is_some() => out.push_str("\\ufffd"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
