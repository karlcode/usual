use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

thread_local! {
    static LOOKED: RefCell<Option<HashSet<String>>> = const { RefCell::new(None) };
}

pub fn note(path: &str) {
    if is_abs(path) {
        LOOKED.with(|l| {
            if let Some(s) = l.borrow_mut().as_mut() {
                s.insert(path.to_string());
            }
        });
    }
}

pub fn swap_looked(new: Option<HashSet<String>>) -> Option<HashSet<String>> {
    LOOKED.with(|l| std::mem::replace(&mut *l.borrow_mut(), new))
}

// Bytes that aren't UTF-8 ride along as U+10FF00 + byte and are written back as the byte.

const RAW: u32 = 0x10FF00;

pub fn text(mut b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len());
    loop {
        match std::str::from_utf8(b) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let ok = e.valid_up_to();
                out.push_str(unsafe { std::str::from_utf8_unchecked(&b[..ok]) });
                out.push(char::from_u32(RAW + b[ok] as u32).unwrap());
                b = &b[ok + 1..];
            }
        }
    }
}

pub fn raw_byte(c: char) -> Option<u8> {
    let c = c as u32;
    if (RAW..=RAW + 0xff).contains(&c) {
        Some((c - RAW) as u8)
    } else {
        None
    }
}

pub fn bytes(s: &str) -> Vec<u8> {
    if !s.chars().any(|c| raw_byte(c).is_some()) {
        return s.as_bytes().to_vec();
    }
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        match raw_byte(c) {
            Some(b) => out.push(b),
            None => out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    out
}

pub fn go_runes(s: &str) -> String {
    s.chars().map(|c| if raw_byte(c).is_some() { '\u{FFFD}' } else { c }).collect()
}

pub fn os(p: &str) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes(p)).into()
}

pub fn os_text(o: &std::ffi::OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    text(o.as_bytes())
}

pub fn getenv(k: &str) -> String {
    std::env::var_os(k).map(|v| os_text(&v)).unwrap_or_default()
}

pub fn home_dir() -> Option<String> {
    let h = getenv("HOME");
    if h.is_empty() {
        None
    } else {
        Some(h)
    }
}

pub fn stat(path: &str) -> Option<fs::Metadata> {
    if path.is_empty() {
        return None;
    }
    fs::metadata(os(path)).ok()
}

pub fn exists(path: &str) -> bool {
    note(path);
    stat(path).is_some()
}

pub fn first_existing(dir: &str, names: &[&str]) -> String {
    for n in names {
        let p = join(&[dir, n]);
        if exists(&p) {
            return p;
        }
    }
    String::new()
}

pub fn is_regular(md: &fs::Metadata) -> bool {
    md.file_type().is_file()
}

pub fn executable_bits(md: &fs::Metadata) -> bool {
    md.permissions().mode() & 0o111 != 0
}

extern "C" {
    fn getuid() -> u32;
    fn faccessat(dirfd: i32, path: *const std::os::raw::c_char, mode: i32, flags: i32) -> i32;
}

pub fn uid() -> u32 {
    unsafe { getuid() }
}

#[cfg(target_os = "linux")]
const AT_FDCWD: i32 = -100;
#[cfg(target_os = "linux")]
const AT_EACCESS: i32 = 0x200;
#[cfg(target_os = "linux")]
const ENOSYS: i32 = 38;
#[cfg(not(target_os = "linux"))]
const AT_FDCWD: i32 = -2;
#[cfg(not(target_os = "linux"))]
const AT_EACCESS: i32 = 0x10;
#[cfg(not(target_os = "linux"))]
const ENOSYS: i32 = 78;
const EPERM: i32 = 1;

fn find_executable(path: &str) -> bool {
    let md = match stat(path) {
        Some(m) => m,
        None => return false,
    };
    if md.is_dir() {
        return false;
    }
    let c = match std::ffi::CString::new(bytes(path)) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if unsafe { faccessat(AT_FDCWD, c.as_ptr(), 1, AT_EACCESS) } == 0 {
        return true;
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(e) if e == ENOSYS || e == EPERM => executable_bits(&md),
        _ => false,
    }
}

pub fn look_path(prog: &str) -> bool {
    for dir in split_list(&getenv("PATH")) {
        let dir = if dir.is_empty() { "." } else { dir };
        let p = join(&[dir, prog]);
        if find_executable(&p) {
            return is_abs(&p);
        }
    }
    false
}

pub fn split_list(s: &str) -> Vec<&str> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(':').collect()
    }
}

pub fn is_abs(p: &str) -> bool {
    p.starts_with('/')
}

pub fn clean(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let p = path.as_bytes();
    let rooted = p[0] == b'/';
    let n = p.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let (mut r, mut dotdot) = (0, 0);
    if rooted {
        out.push(b'/');
        r = 1;
        dotdot = 1;
    }
    while r < n {
        if p[r] == b'/' || p[r] == b'.' && (r + 1 == n || p[r + 1] == b'/') {
            r += 1;
        } else if p[r] == b'.' && p[r + 1] == b'.' && (r + 2 == n || p[r + 2] == b'/') {
            r += 2;
            if out.len() > dotdot {
                let mut w = out.len() - 1;
                while w > dotdot && out[w] != b'/' {
                    w -= 1;
                }
                out.truncate(w);
            } else if !rooted {
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(b"..");
                dotdot = out.len();
            }
        } else {
            if rooted && out.len() != 1 || !rooted && !out.is_empty() {
                out.push(b'/');
            }
            while r < n && p[r] != b'/' {
                out.push(p[r]);
                r += 1;
            }
        }
    }
    if out.is_empty() {
        return ".".into();
    }
    String::from_utf8(out).unwrap_or_default()
}

pub fn join(elems: &[&str]) -> String {
    let parts: Vec<&str> = elems.iter().copied().skip_while(|e| e.is_empty()).collect();
    if parts.is_empty() {
        return String::new();
    }
    clean(&parts.join("/"))
}

pub fn dir(path: &str) -> String {
    let i = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    clean(&path[..i])
}

pub fn base(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let p = path.trim_end_matches('/');
    if p.is_empty() {
        return "/".into();
    }
    match p.rfind('/') {
        Some(i) => p[i + 1..].to_string(),
        None => p.to_string(),
    }
}

pub fn abs(path: &str) -> Option<String> {
    if is_abs(path) {
        return Some(clean(path));
    }
    Some(join(&[&getwd()?, path]))
}

fn getwd() -> Option<String> {
    let pwd = getenv("PWD");
    if is_abs(&pwd) {
        if let (Some(a), Some(b)) = (stat(&pwd), stat(".")) {
            if a.dev() == b.dev() && a.ino() == b.ino() {
                return Some(pwd);
            }
        }
    }
    std::env::current_dir().ok().map(|p| os_text(p.as_os_str()))
}

fn has_meta(p: &str) -> bool {
    p.contains(['*', '?', '[', '\\'])
}

pub fn glob(pattern: &str) -> Vec<String> {
    if match_pattern(pattern, "").is_err() {
        return Vec::new();
    }
    if !has_meta(pattern) {
        return if fs::symlink_metadata(os(pattern)).is_ok() { vec![pattern.to_string()] } else { Vec::new() };
    }
    let (d, file) = match pattern.rfind('/') {
        Some(i) => (&pattern[..i + 1], &pattern[i + 1..]),
        None => ("", pattern),
    };
    let d = if d.is_empty() {
        ".".to_string()
    } else if d == "/" {
        d.to_string()
    } else {
        d[..d.len() - 1].to_string()
    };
    let mut out = Vec::new();
    if !has_meta(&d) {
        glob_in(&d, file, &mut out);
        return out;
    }
    if d == pattern {
        return Vec::new();
    }
    for m in glob(&d) {
        glob_in(&m, file, &mut out);
    }
    out
}

fn glob_in(d: &str, pattern: &str, out: &mut Vec<String>) {
    match stat(d) {
        Some(m) if m.is_dir() => {}
        _ => return,
    }
    let rd = match fs::read_dir(os(d)) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| os_text(&e.file_name())).collect();
    names.sort();
    for n in names {
        match match_pattern(pattern, &n) {
            Err(_) => return,
            Ok(true) => out.push(join(&[d, &n])),
            Ok(false) => {}
        }
    }
}

pub fn match_pattern(pattern: &str, name: &str) -> Result<bool, ()> {
    let mut pattern = pattern;
    let mut name = name;
    'pattern: while !pattern.is_empty() {
        let (star, chunk, rest) = scan_chunk(pattern);
        pattern = rest;
        if star && chunk.is_empty() {
            return Ok(!name.contains('/'));
        }
        let (t, ok) = match_chunk(chunk, name)?;
        if ok && (t.is_empty() || !pattern.is_empty()) {
            name = t;
            continue;
        }
        if star {
            let mut i = 0;
            while i < name.len() && name.as_bytes()[i] != b'/' {
                let skip = first_char(&name[i..]).1;
                let (t, ok) = match_chunk(chunk, &name[i + skip..])?;
                if ok {
                    if pattern.is_empty() && !t.is_empty() {
                        i += skip;
                        continue;
                    }
                    name = t;
                    continue 'pattern;
                }
                i += skip;
            }
        }
        while !pattern.is_empty() {
            let (_, chunk, rest) = scan_chunk(pattern);
            pattern = rest;
            match_chunk(chunk, "")?;
        }
        return Ok(false);
    }
    Ok(name.is_empty())
}

fn scan_chunk(mut pattern: &str) -> (bool, &str, &str) {
    let mut star = false;
    while pattern.starts_with('*') {
        pattern = &pattern[1..];
        star = true;
    }
    let b = pattern.as_bytes();
    let mut inrange = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                if i + 1 < b.len() {
                    i += 1;
                }
            }
            b'[' => inrange = true,
            b']' => inrange = false,
            b'*' if !inrange => break,
            _ => {}
        }
        i += 1;
    }
    (star, &pattern[..i], &pattern[i..])
}

fn first_char(s: &str) -> (char, usize) {
    let c = s.chars().next().unwrap_or('\u{FFFD}');
    (c, c.len_utf8())
}

fn match_chunk<'a>(chunk: &str, s: &'a str) -> Result<(&'a str, bool), ()> {
    let mut chunk = chunk;
    let mut s = s;
    let mut failed = false;
    while !chunk.is_empty() {
        if !failed && s.is_empty() {
            failed = true;
        }
        match chunk.as_bytes()[0] {
            b'[' => {
                let mut r = '\0';
                if !failed {
                    let (c, n) = first_char(s);
                    r = c;
                    s = &s[n..];
                }
                chunk = &chunk[1..];
                let mut negated = false;
                if chunk.starts_with('^') {
                    negated = true;
                    chunk = &chunk[1..];
                }
                let mut matched = false;
                let mut nrange = 0;
                loop {
                    if !chunk.is_empty() && chunk.as_bytes()[0] == b']' && nrange > 0 {
                        chunk = &chunk[1..];
                        break;
                    }
                    let (lo, rest) = get_esc(chunk)?;
                    chunk = rest;
                    let mut hi = lo;
                    if chunk.starts_with('-') {
                        let (h, rest) = get_esc(&chunk[1..])?;
                        hi = h;
                        chunk = rest;
                    }
                    if lo <= r && r <= hi {
                        matched = true;
                    }
                    nrange += 1;
                }
                if matched == negated {
                    failed = true;
                }
            }
            b'?' => {
                if !failed {
                    if s.starts_with('/') {
                        failed = true;
                    }
                    let (_, n) = first_char(s);
                    s = &s[n.min(s.len())..];
                }
                chunk = &chunk[1..];
            }
            b'\\' => {
                chunk = &chunk[1..];
                if chunk.is_empty() {
                    return Err(());
                }
                let (c, n) = first_char(chunk);
                if !failed {
                    let (d, m) = first_char(s);
                    if c != d {
                        failed = true;
                    }
                    s = &s[m..];
                }
                chunk = &chunk[n..];
            }
            _ => {
                let (c, n) = first_char(chunk);
                if !failed {
                    let (d, m) = first_char(s);
                    if c != d {
                        failed = true;
                    }
                    s = &s[m..];
                }
                chunk = &chunk[n..];
            }
        }
    }
    if failed {
        return Ok(("", false));
    }
    Ok((s, true))
}

fn get_esc(chunk: &str) -> Result<(char, &str), ()> {
    if chunk.is_empty() || chunk.starts_with('-') || chunk.starts_with(']') {
        return Err(());
    }
    let mut chunk = chunk;
    if chunk.starts_with('\\') {
        chunk = &chunk[1..];
        if chunk.is_empty() {
            return Err(());
        }
    }
    let (r, n) = first_char(chunk);
    let rest = &chunk[n..];
    if rest.is_empty() {
        return Err(());
    }
    Ok((r, rest))
}
