use crate::gofs::{getenv, home_dir, join, note};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::{FileExt, OpenOptionsExt};

// The history file: one "unix-time<TAB>root<TAB>command" line per command run.

pub(crate) const HISTORY_TAIL: u64 = 256 << 10;
pub(crate) const HISTORY_MAX: u64 = 1 << 20;
const HOUR_NS: i64 = 3_600_000_000_000;
const SESSION_GAP: i64 = HOUR_NS;
const HALF_LIFE_HOURS: f64 = 168.0;
const HISTORY_SCORE: i64 = 2000;
const FOLLOWER_SCORE: i64 = 3000;

pub type Nanos = i128;

pub fn now() -> Nanos {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as i128).unwrap_or(0)
}

pub fn unix(t: Nanos) -> i64 {
    t.div_euclid(1_000_000_000) as i64
}

fn sub(t: Nanos, u: Option<Nanos>) -> i64 {
    match u {
        None => i64::MAX, // u is the zero time, two thousand years ago
        Some(u) => (t - u).clamp(i64::MIN as i128, i64::MAX as i128) as i64,
    }
}

fn hours(d: i64) -> f64 {
    (d / HOUR_NS) as f64 + (d % HOUR_NS) as f64 / (60.0 * 60.0 * 1e9)
}

pub fn history_path() -> String {
    let mut dir = getenv("XDG_STATE_HOME");
    if dir.is_empty() {
        match home_dir() {
            Some(h) => dir = join(&[&h, ".local", "state"]),
            None => return String::new(),
        }
    }
    join(&[&dir, "usual", "history"])
}

#[derive(Default)]
pub struct ProjectUse {
    follows: HashMap<String, i64>,
    recency: HashMap<String, f64>,
    last: Option<Nanos>,
}

impl ProjectUse {
    pub fn expires(&self, now: Nanos) -> Nanos {
        if let Some(last) = self.last {
            let end = last + SESSION_GAP as i128;
            if end > now {
                return end;
            }
        }
        now + HOUR_NS as i128
    }

    pub fn score(&self, cmd: &str) -> i64 {
        let n = self.follows.get(cmd).copied().unwrap_or(0);
        let r = self.recency.get(cmd).copied().unwrap_or(0.0);
        if n > 0 {
            FOLLOWER_SCORE + n.min(9) * 100 + ((r * 10.0) as i64).min(99)
        } else if r > 0.0 {
            HISTORY_SCORE + ((r * 10.0) as i64).min(999)
        } else {
            0
        }
    }
}

pub fn project_usage(root: &str, now: Nanos) -> ProjectUse {
    let mut entries: Vec<(Nanos, String)> = Vec::new();
    let tail = read_tail(&history_path());
    for l in tail.split('\n') {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 3 || f[1] != root || f[2].is_empty() {
            continue;
        }
        if let Ok(sec) = f[0].parse::<i64>() {
            entries.push((sec as i128 * 1_000_000_000, f[2].to_string()));
        }
    }
    let mut u = ProjectUse::default();
    if entries.is_empty() {
        return u;
    }
    const START: &str = "\x00start";
    let mut transitions: HashMap<String, HashMap<String, i64>> = HashMap::new();
    let mut prev = START.to_string();
    let mut last: Option<Nanos> = None;
    for (at, cmd) in entries {
        if sub(at, last) > SESSION_GAP {
            prev = START.to_string();
        }
        *transitions.entry(prev.clone()).or_default().entry(cmd.clone()).or_default() += 1;
        *u.recency.entry(cmd.clone()).or_default() += 0.5f64.powf(hours(sub(now, Some(at))) / HALF_LIFE_HOURS);
        prev = cmd;
        last = Some(at);
    }
    if sub(now, last) > SESSION_GAP {
        prev = START.to_string();
    }
    u.follows = transitions.remove(&prev).unwrap_or_default();
    u.last = last;
    u
}

pub(crate) fn read_tail(path: &str) -> String {
    note(path);
    if path.is_empty() {
        return String::new();
    }
    let f = match fs::File::open(crate::gofs::os(path)) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let size = match f.metadata() {
        Ok(m) => m.len(),
        Err(_) => return String::new(),
    };
    let off = size.saturating_sub(HISTORY_TAIL);
    let mut buf = vec![0u8; (size - off) as usize];
    let mut done = 0;
    while done < buf.len() {
        match f.read_at(&mut buf[done..], off + done as u64) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(_) => return String::new(),
        }
    }
    let mut s = crate::gofs::text(&buf);
    if off > 0 {
        s = match s.split_once('\n') {
            Some((_, after)) => after.to_string(),
            None => String::new(),
        };
    }
    if size > HISTORY_MAX {
        trim(path, &s);
    }
    s
}

fn trim(path: &str, tail: &str) {
    let tmp = format!("{path}.tmp");
    let ok = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(crate::gofs::os(&tmp))
        .and_then(|mut f| std::io::Write::write_all(&mut f, &crate::gofs::bytes(tail)))
        .is_ok();
    if ok {
        let _ = fs::rename(crate::gofs::os(&tmp), crate::gofs::os(path));
    }
}
