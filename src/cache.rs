use crate::gofs::{bytes, dir, getenv, home_dir, is_abs, join, note, os, stat, swap_looked, text};
use crate::history::{unix, Nanos};
use crate::one_line;
use crate::suggest::{discover_fresh, scan_lines, Candidate};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};

// A dependency is stamped "mtime.size" as zstat sees it; a missing path stands as its nearest folder.
// Anything modified in the stamp's own second gets "?", which never matches.

pub(crate) const CACHE_FORMAT: &str = "usual-cache 2";
pub(crate) const HINTS_FORMAT: &str = "usual-hints 2";

fn unix_now() -> i64 {
    unix(crate::history::now())
}

fn stamp_of(md: &fs::Metadata, now: i64) -> String {
    if md.mtime() >= now {
        return "?".into();
    }
    format!("{}.{}", md.mtime(), md.size())
}

pub fn dependencies(paths: &HashSet<String>) -> HashMap<String, String> {
    let now = unix_now();
    let mut deps: HashMap<String, String> = HashMap::new();
    for p in paths {
        let mut p = p.clone();
        loop {
            if deps.contains_key(&p) {
                break;
            }
            if let Some(md) = stat(&p) {
                deps.insert(p, stamp_of(&md, now));
                break;
            }
            let parent = dir(&p);
            if parent == p {
                deps.insert(p, "-".into());
                break;
            }
            p = parent;
        }
    }
    deps
}

pub(crate) fn unchanged(path: &str, stamp: &str) -> bool {
    match stat(path) {
        None => stamp == "-",
        Some(md) => stamp_of(&md, unix_now() + 1) == stamp,
    }
}

pub fn cache_dir() -> String {
    let d = getenv("XDG_CACHE_HOME");
    if !d.is_empty() {
        return join(&[&d, "usual"]);
    }
    match home_dir() {
        Some(h) => join(&[&h, ".cache", "usual"]),
        None => String::new(),
    }
}

pub(crate) fn build() -> String {
    let v = crate::VERSION;
    match std::env::current_exe().ok().and_then(|e| fs::metadata(e).ok()) {
        Some(md) => format!("{v} {}", stamp_of(&md, unix_now() + 1)),
        None => v.to_string(),
    }
}

pub fn discover(root: &str) -> Vec<Candidate> {
    let path = discovery_path(root);
    if let Some((cs, deps)) = load_discovery(&path, root) {
        for p in deps {
            note(&p);
        }
        return cs;
    }
    let outer = swap_looked(Some(HashSet::new()));
    let cs = discover_fresh(root);
    let paths = swap_looked(outer).unwrap_or_default();
    for p in &paths {
        note(p);
    }
    save_discovery(&path, root, &dependencies(&paths), &cs);
    cs
}

pub(crate) fn discovery_path(root: &str) -> String {
    let d = cache_dir();
    if d.is_empty() {
        return String::new();
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for b in root.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    join(&[&d, "projects", &base36(h)])
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

fn load_discovery(path: &str, root: &str) -> Option<(Vec<Candidate>, Vec<String>)> {
    if path.is_empty() {
        return None;
    }
    let data = fs::read(os(path)).ok()?;
    let lines = scan_lines(&data);
    let mut it = lines.iter().map(|l| text(l));
    if it.next()? != format!("{CACHE_FORMAT} {}", build()) || it.next()? != root {
        return None;
    }
    let mut cs = Vec::new();
    let mut deps = Vec::new();
    for l in it {
        if l.len() >= 1 << 20 {
            return None;
        }
        let f: Vec<&str> = l.split('\t').collect();
        match (f[0], f.len()) {
            ("P", 3) => {
                if !unchanged(f[1], f[2]) {
                    return None;
                }
                deps.push(f[1].to_string());
            }
            ("C", 10) => {
                let mut n = [0i64; 5];
                for (i, s) in [f[3], f[4], f[7], f[8], f[9]].iter().enumerate() {
                    n[i] = s.parse().ok()?;
                }
                cs.push(Candidate {
                    cmd: f[1].into(),
                    desc: f[2].into(),
                    score: n[0],
                    stage: n[1],
                    known: f[5] == "1",
                    family: f[6].into(),
                    dev_pos: n[2],
                    other_pos: n[3],
                    check_pos: n[4],
                    ..Default::default()
                });
            }
            _ => return None,
        }
    }
    Some((cs, deps))
}

fn save_discovery(path: &str, root: &str, deps: &HashMap<String, String>, cs: &[Candidate]) {
    if path.is_empty() || root.contains(['\t', '\n']) {
        return;
    }
    let mut b = format!("{CACHE_FORMAT} {}\n{root}\n", build());
    for (p, s) in deps {
        if p.contains(['\t', '\n']) {
            return;
        }
        b += &format!("P\t{p}\t{s}\n");
    }
    for c in cs {
        b += &format!(
            "C\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            one_line(&c.cmd),
            one_line(&c.desc),
            c.score,
            c.stage,
            if c.known { "1" } else { "0" },
            one_line(&c.family),
            c.dev_pos,
            c.other_pos,
            c.check_pos
        );
    }
    write_atomic(path, &bytes(&b));
}

pub(crate) fn hints_path(d: &str) -> String {
    let b = cache_dir();
    if b.is_empty() || !is_abs(d) {
        return String::new();
    }
    join(&[&b, "hints", d, ".hint"])
}

pub fn save_hints(d: &str, out: &[u8], deps: &HashMap<String, String>, expires: Nanos) {
    let path = hints_path(d);
    if path.is_empty() {
        return;
    }
    let mut deps = deps.clone();
    if let Some(exe) = std::env::current_exe().ok().and_then(|e| e.to_str().map(String::from)) {
        if let Ok(md) = fs::metadata(&exe) {
            deps.insert(exe, stamp_of(&md, unix_now()));
        }
    }
    let b = format!("{HINTS_FORMAT}\n{}\n{}\n{}\n", unix(expires), getenv("PATH"), deps.len());
    let mut b = bytes(&b);
    for (p, s) in &deps {
        if p.contains('\n') {
            return;
        }
        b.extend_from_slice(&bytes(&format!("{p}\n{s}\n")));
    }
    b.extend_from_slice(out);
    write_atomic(&path, &b);
}

pub fn mkdir_all(path: &str) -> bool {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(os(path)).is_ok()
}

fn write_atomic(path: &str, data: &[u8]) {
    if !mkdir_all(&dir(path)) {
        return;
    }
    let tmp = format!("{path}.{}.tmp", std::process::id());
    let ok = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(os(&tmp))
        .and_then(|mut f| f.write_all(data))
        .is_ok()
        && fs::rename(os(&tmp), os(path)).is_ok();
    if !ok {
        let _ = fs::remove_file(os(&tmp));
    }
}
