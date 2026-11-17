// Hand-written matchers: the regex crate cost over 1 ms per call, more than a whole cache hit.

fn is_ws(c: u8) -> bool {
    matches!(c, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    i
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn ident(b: &[u8], i: usize, extra: &[u8]) -> Option<usize> {
    if i >= b.len() || !is_ident_start(b[i]) {
        return None;
    }
    let mut j = i + 1;
    while j < b.len() && (is_word(b[j]) || extra.contains(&b[j])) {
        j += 1;
    }
    Some(j)
}

fn fold(s: &str) -> Vec<char> {
    s.chars()
        .map(|c| match c {
            'ſ' => 's',
            '\u{212A}' => 'k',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

fn has_at(h: &[char], i: usize, w: &str) -> bool {
    w.chars().enumerate().all(|(k, c)| h.get(i + k) == Some(&c))
}

/// `(?i)develop|contribut|getting started|set ?up|install|build|running|local|quick ?start|hacking|test`
pub fn dev_heading(s: &str) -> bool {
    const WORDS: &[&str] = &[
        "develop",
        "contribut",
        "getting started",
        "setup",
        "set up",
        "install",
        "build",
        "running",
        "local",
        "quickstart",
        "quick start",
        "hacking",
        "test",
    ];
    let h = fold(s);
    (0..h.len()).any(|i| WORDS.iter().any(|w| has_at(&h, i, w)))
}

/// `(?i)^(ci|test|tests|build|lint|check|checks|main|pr|pull|verify|validate)[^a-z]`
pub fn ci_workflow(s: &str) -> bool {
    const WORDS: &[&str] =
        &["ci", "test", "tests", "build", "lint", "check", "checks", "main", "pr", "pull", "verify", "validate"];
    let h = fold(s);
    WORDS.iter().any(|w| {
        let n = w.chars().count();
        has_at(&h, 0, w) && h.get(n).is_some_and(|c| !c.is_ascii_lowercase())
    })
}

/// `^(#|echo\b|printf\b|...|mkdir\b)`: recipe lines that aren't the command.
pub fn recipe_noise(s: &str) -> bool {
    const WORDS: &[&str] =
        &["echo", "printf", "if", "for", "while", "case", "then", "else", "fi", "do", "done", "set", "test", "mkdir"];
    const PREFIXES: &[&str] = &["#", "[", "$(call ", "$(info ", "$(eval ", "$(Q)echo"];
    let b = s.as_bytes();
    PREFIXES.iter().any(|p| s.starts_with(p))
        || WORDS.iter().any(|w| s.starts_with(w) && !b.get(w.len()).is_some_and(|&c| is_word(c)))
}

/// `^(\./)?[A-Za-z0-9][A-Za-z0-9 :_./@+=,~-]*$`
pub fn safe_command(s: &str) -> bool {
    let b = s.as_bytes();
    let rest = b.strip_prefix(b"./").filter(|r| r.first().is_some_and(|c| c.is_ascii_alphanumeric())).unwrap_or(b);
    !rest.is_empty()
        && rest[0].is_ascii_alphanumeric()
        && rest.iter().all(|&c| c.is_ascii_alphanumeric() || b" :_./@+=,~-".contains(&c))
}

/// `^[A-Za-z0-9][A-Za-z0-9:_./@+=-]*$`
pub fn safe_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|&c| c.is_ascii_alphanumeric() || b":_./@+=-".contains(&c))
}

/// `^[A-Za-z_][A-Za-z0-9_]*=`
pub fn env_assign(s: &str) -> bool {
    let b = s.as_bytes();
    ident(b, 0, b"").is_some_and(|j| b.get(j) == Some(&b'='))
}

pub fn split_commands(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let (mut piece, mut i) = (0, 0);
    let mut quote = 0u8; // inside '…' or "…" a ; or && is text: FOO="a;b" cmd is one command
    while i < b.len() {
        if quote != 0 {
            if b[i] == b'\\' && quote == b'"' {
                i += 1;
            } else if b[i] == quote {
                quote = 0;
            }
            i += 1;
            continue;
        }
        if b[i] == b'\'' || b[i] == b'"' {
            quote = b[i];
            i += 1;
            continue;
        }
        let sep = if b[i..].starts_with(b"&&") || b[i..].starts_with(b"||") {
            2
        } else if b[i] == b';' {
            1
        } else {
            i += 1;
            continue;
        };
        let mut start = i;
        while start > piece && is_ws(b[start - 1]) {
            start -= 1;
        }
        out.push(&s[piece..start]);
        i = skip_ws(b, i + sep);
        piece = i;
    }
    out.push(&s[piece..]);
    out
}

pub fn inline_code(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(o) = b[i..].iter().position(|&c| c == b'`') {
        let open = i + o;
        match b[open + 1..].iter().position(|&c| c == b'`') {
            None => break,
            Some(0) => i = open + 1,
            Some(n) => {
                out.push(&s[open + 1..open + 1 + n]);
                i = open + 2 + n;
            }
        }
    }
    out
}

/// `^@?([A-Za-z_][A-Za-z0-9_-]*)([^:]*):(.*)$`: a recipe's name and what follows its colon.
pub fn just_recipe(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    let at = usize::from(b.first() == Some(&b'@'));
    let end = ident(b, at, b"-")?;
    let colon = end + b[end..].iter().position(|&c| c == b':')?;
    let rest = &s[colon + 1..];
    if rest.contains('\n') {
        return None;
    }
    Some((&s[at..end], rest))
}

/// `doc\(\s*["'](.*)["']\s*\)`
pub fn just_doc(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut from = 0;
    while let Some(o) = s[from..].find("doc(") {
        let start = from + o;
        from = start + 1;
        let q = skip_ws(b, start + 4);
        if q >= b.len() || !matches!(b[q], b'"' | b'\'') {
            continue;
        }
        // The greedy .* stops at the last quote that ws* ) follows, before any newline.
        let limit = b[q + 1..].iter().position(|&c| c == b'\n').map_or(b.len(), |n| q + 1 + n);
        for k in (q + 1..b.len().min(limit + 1)).rev() {
            if k < b.len() && matches!(b[k], b'"' | b'\'') {
                let p = skip_ws(b, k + 1);
                if b.get(p) == Some(&b')') {
                    return Some(&s[q + 1..k]);
                }
            }
        }
    }
    None
}

/// `^([A-Za-z_][A-Za-z0-9_]*)\s*[:?]?=\s*(.*)$`: a Makefile variable and its value.
pub fn make_assign(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    let end = ident(b, 0, b"")?;
    let mut i = skip_ws(b, end);
    if matches!(b.get(i), Some(b':' | b'?')) && b.get(i + 1) == Some(&b'=') {
        i += 1;
    }
    if b.get(i) != Some(&b'=') {
        return None;
    }
    let v = skip_ws(b, i + 1);
    if s[v..].contains('\n') {
        return None;
    }
    Some((&s[..end], &s[v..]))
}

pub fn replace_make_vars(s: &str, f: impl Fn(&str) -> String) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let (mut last, mut i) = (0, 0);
    while i < b.len() {
        if b[i] == b'$' && matches!(b.get(i + 1), Some(b'(' | b'{')) {
            if let Some(end) = ident(b, i + 2, b"") {
                if matches!(b.get(end), Some(b')' | b'}')) {
                    out.push_str(&s[last..i]);
                    out.push_str(&f(&s[i..end + 1]));
                    last = end + 1;
                    i = end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push_str(&s[last..]);
    out
}

fn mise_name(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"_:.-".contains(&c)
}

/// `^\[tasks\.(?:"([^"]+)"|'([^']+)'|([A-Za-z0-9_:.-]+))\]`: a [tasks.name] section's name.
pub fn mise_section(s: &str) -> Option<&str> {
    let rest = s.strip_prefix("[tasks.")?;
    let b = rest.as_bytes();
    match *b.first()? {
        q @ (b'"' | b'\'') => {
            let n = b[1..].iter().position(|&c| c == q)?;
            (n > 0 && b.get(n + 2) == Some(&b']')).then(|| &rest[1..n + 1])
        }
        _ => {
            let n = b.iter().position(|&c| !mise_name(c)).unwrap_or(b.len());
            (n > 0 && b.get(n) == Some(&b']')).then(|| &rest[..n])
        }
    }
}

/// `^["']?([A-Za-z0-9_:.-]+)["']?\s*=\s*(.*)$`: a key under [tasks] and its value.
pub fn mise_key(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    let start = usize::from(matches!(b.first(), Some(b'"' | b'\'')));
    let mut end = start;
    while end < b.len() && mise_name(b[end]) {
        end += 1;
    }
    if end == start {
        return None;
    }
    let mut i = end;
    if matches!(b.get(i), Some(b'"' | b'\'')) {
        i += 1;
    }
    i = skip_ws(b, i);
    if b.get(i) != Some(&b'=') {
        return None;
    }
    let v = skip_ws(b, i + 1);
    if s[v..].contains('\n') {
        return None;
    }
    Some((&s[start..end], &s[v..]))
}

/// `KEY\s*=\s*["']([^"']*)["']` at i.
fn quoted_value_at(s: &str, i: usize) -> Option<&str> {
    let b = s.as_bytes();
    let i = skip_ws(b, i);
    if b.get(i) != Some(&b'=') {
        return None;
    }
    let q = skip_ws(b, i + 1);
    if !matches!(b.get(q), Some(b'"' | b'\'')) {
        return None;
    }
    let n = b[q + 1..].iter().position(|&c| c == b'"' || c == b'\'')?;
    Some(&s[q + 1..q + 1 + n])
}

/// `description\s*=\s*["']([^"']*)["']`
pub fn mise_desc(s: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(o) = s[from..].find("description") {
        let at = from + o;
        if let Some(v) = quoted_value_at(s, at + "description".len()) {
            return Some(v);
        }
        from = at + 1;
    }
    None
}

/// `^run\s*=\s*["']([^"']*)["']`
pub fn mise_run(s: &str) -> Option<&str> {
    s.strip_prefix("run")?;
    quoted_value_at(s, 3)
}
