use crate::cache::discover;
use crate::evidence::{check_commands, setup_commands};
use crate::fields;
use crate::gofs::{
    clean, dir as dir_of, executable_bits, exists, first_existing, home_dir, is_regular, join, look_path, note, stat,
    uid,
};
use crate::history::{project_usage, Nanos, ProjectUse};
use crate::json;
use crate::sources::{read_file, SOURCES};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Default, Debug)]
pub struct Suggestion {
    pub cmd: String,
    pub desc: String,
}

pub fn suggest(dir: &str, now: Nanos) -> (String, Vec<Suggestion>, Nanos) {
    let root = find_root(dir);
    if root.is_empty() {
        return (root, Vec::new(), now + 7 * 24 * 3_600_000_000_000);
    }
    let mut cs = discover(&root);
    let used = project_usage(&root, now);
    rank(&mut cs, &used);
    let expires = used.expires(now);
    let mut seen: HashSet<String> = HashSet::new();
    let mut found: HashMap<String, bool> = HashMap::new();
    let mut list = Vec::new();
    for c in cs {
        if seen.contains(&c.cmd) || seen.contains(&format!("wraps {}", c.desc)) || !crate::pat::safe_command(&c.cmd) {
            continue;
        }
        seen.insert(c.cmd.clone());
        seen.insert(format!("wraps {}", c.cmd));
        if !c.desc.is_empty() {
            seen.insert(c.desc.clone());
        }
        let prog = c.cmd.split(' ').next().unwrap_or("").to_string();
        if dir != root && root_only(&prog) {
            continue;
        }
        let ok = *found.entry(prog.clone()).or_insert_with(|| installed(&root, &prog));
        if ok {
            list.push(Suggestion { cmd: c.cmd, desc: c.desc });
        }
    }
    (root, list, expires)
}

fn root_only(prog: &str) -> bool {
    prog == "make" || prog == "go" || prog == "cp" || prog == "git" || prog.contains('/')
}

fn installed(root: &str, prog: &str) -> bool {
    if prog.contains('/') {
        let p = join(&[root, prog]);
        note(&p);
        return matches!(stat(&p), Some(m) if is_regular(&m) && executable_bits(&m));
    }
    look_path(prog)
}

pub fn discover_fresh(root: &str) -> Vec<Candidate> {
    let mut cs: Vec<Candidate> = Vec::new();
    for (_, read) in SOURCES {
        cs.extend(read(root));
    }
    let clis = project_clis(root);
    let mut progs = programs(&cs);
    progs.extend(clis.iter().cloned());
    let (dev, contrib, rest, checks) = doc_commands(root, &progs);
    let lines: Vec<String> = dev.iter().chain(&contrib).chain(&checks).cloned().collect();
    cs.extend(cli_commands(&lines, &clis));
    let other: Vec<String> = contrib.iter().chain(&rest).cloned().collect();
    let (dev_pos, other_pos, check_pos) = (mentions(&cs, &dev), mentions(&cs, &other), mentions(&cs, &checks));
    for (i, c) in cs.iter_mut().enumerate() {
        c.dev_pos = dev_pos[i];
        c.other_pos = other_pos[i];
        c.check_pos = check_pos[i];
        c.spelled_out(&dev);
    }
    cs
}

impl Candidate {
    fn spelled_out(&mut self, dev: &[String]) {
        if self.verbs.is_empty() || self.dev_pos == 0 {
            return;
        }
        let l = &dev[(self.dev_pos - 1) as usize];
        let line = l.strip_prefix("./").unwrap_or(l);
        for v in &self.verbs {
            if line.starts_with(&format!("{v} ")) && crate::pat::safe_command(line) && line.len() <= 80 {
                self.cmd = line.to_string();
            }
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct Candidate {
    pub cmd: String,
    pub desc: String,
    pub score: i64,
    pub forms: Vec<String>,
    pub family: String,
    pub stage: i64,
    pub known: bool,
    pub verbs: Vec<String>,
    pub dev_pos: i64,
    pub other_pos: i64,
    pub check_pos: i64,
}

pub const SETUP_SCORE: i64 = 10000;
const STAGE_DOCS: i64 = 1500;
pub const STAGE_RUN: i64 = 1000;
pub const STAGE_SETUP: i64 = 800;
pub const STAGE_BUILD: i64 = 600;
pub const STAGE_VERIFY: i64 = 400;
pub const STAGE_OTHER: i64 = 200;
pub const STAGE_MINOR: i64 = 100;
pub const STAGE_RISKY: i64 = -100;
const CHECK_BONUS: i64 = 8;

fn stages(w: &str) -> Option<(i64, i64)> {
    Some(match w {
        "dev" => (STAGE_RUN, 60),
        "start" => (STAGE_RUN, 58),
        "serve" | "run" => (STAGE_RUN, 55),
        "up" => (STAGE_RUN, 50),
        "watch" => (STAGE_RUN, 40),
        "preview" => (STAGE_RUN, 30),
        "setup" | "bootstrap" => (STAGE_SETUP, 50),
        "build" => (STAGE_BUILD, 50),
        "compile" => (STAGE_BUILD, 45),
        "all" => (STAGE_BUILD, 40),
        "test" => (STAGE_VERIFY, 50),
        "unit" | "spec" => (STAGE_VERIFY, 45),
        "e2e" => (STAGE_VERIFY, 42),
        "integration" => (STAGE_VERIFY, 40),
        "acceptance" => (STAGE_VERIFY, 38),
        "check" | "verify" => (STAGE_VERIFY, 40),
        "validate" => (STAGE_VERIFY, 36),
        "lint" => (STAGE_VERIFY, 35),
        "eslint" | "vet" | "clippy" => (STAGE_VERIFY, 33),
        "typecheck" | "tc" | "tsc" | "types" | "type" => (STAGE_VERIFY, 30),
        "format" | "fmt" | "prettier" => (STAGE_VERIFY, 25),
        "fix" => (STAGE_VERIFY, 22),
        "stylelint" => (STAGE_VERIFY, 20),
        "knip" => (STAGE_VERIFY, 15),
        _ => return None,
    })
}

fn minor(w: &str) -> bool {
    matches!(
        w,
        "ci" | "cov"
            | "cover"
            | "coverage"
            | "tap"
            | "help"
            | "changelog"
            | "profile"
            | "bench"
            | "benchmark"
            | "inspect"
            | "debug"
            | "prod"
            | "production"
    )
}

fn risky(w: &str) -> bool {
    matches!(
        w,
        "deploy"
            | "publish"
            | "release"
            | "clean"
            | "clear"
            | "reset"
            | "drop"
            | "destroy"
            | "nuke"
            | "purge"
            | "prune"
            | "wipe"
            | "rollback"
            | "teardown"
            | "down"
            | "kill"
            | "rm"
            | "uninstall"
            | "bump"
            | "push"
            | "ship"
            | "changeset"
    )
}

fn cautious(w: &str) -> bool {
    w == "migrate" || w == "seed"
}

pub fn setup(cmd: &str) -> Candidate {
    Candidate {
        cmd: cmd.into(),
        desc: "dependencies not installed".into(),
        score: SETUP_SCORE,
        stage: SETUP_SCORE,
        known: true,
        ..Default::default()
    }
}

pub fn task(cmd: &str, desc: &str, name: &str, forms: &[&str]) -> Candidate {
    let cmd = if crate::pat::safe_name(name) { cmd.to_string() } else { String::new() };
    let mut family = go_lower(name);
    if let Some(i) = family.find(|c| NAME_SEPARATORS.contains(c)) {
        if i > 0 {
            family.truncate(i);
        }
    }
    let (stage, score, known) = name_score(name);
    let mut fs: Vec<String> = forms.iter().map(|s| s.to_string()).collect();
    fs.push(cmd.clone());
    Candidate { cmd, desc: desc.into(), score, forms: fs, family, stage, known, ..Default::default() }
}

fn go_lower(s: &str) -> String {
    if s.is_ascii() {
        s.to_ascii_lowercase()
    } else {
        crate::gofs::go_runes(s).to_lowercase()
    }
}

const NAME_SEPARATORS: &str = ":-_ ./";

pub fn name_score(name: &str) -> (i64, i64, bool) {
    let lower = go_lower(name);
    let words: Vec<&str> = lower.split(|c| NAME_SEPARATORS.contains(c)).filter(|w| !w.is_empty()).collect();
    if words.iter().any(|w| risky(w)) {
        return (STAGE_RISKY, STAGE_RISKY, true);
    }
    if words.iter().any(|w| cautious(w)) {
        return (STAGE_RISKY, STAGE_RISKY, false);
    }
    if words.iter().any(|w| minor(w)) {
        return (STAGE_MINOR, STAGE_MINOR, true);
    }
    let st = words.first().and_then(|w| stages(w));
    match st {
        Some(mut st) if !(words.len() > 1 && (words[0] == "run" || words[0] == "up")) => {
            if words.len() > 1 {
                st.1 -= 5;
            }
            (st.0, st.0 + st.1, true)
        }
        _ => (STAGE_OTHER, STAGE_OTHER + 10, false),
    }
}

pub fn rank(cs: &mut Vec<Candidate>, used: &ProjectUse) {
    let mut singled = vec![false; cs.len()];
    for (i, c) in cs.iter_mut().enumerate() {
        if c.score >= SETUP_SCORE {
            singled[i] = true;
            continue;
        }
        let u = used.score(&c.cmd);
        if u > 0 {
            c.score = u;
            singled[i] = true;
            continue;
        }
        if c.dev_pos > 0 {
            if c.known && c.stage <= STAGE_VERIFY {
                c.score = c.stage + 61 + 40 - (c.dev_pos - 1).min(40);
            } else {
                c.score = STAGE_DOCS - c.dev_pos.min(400);
                singled[i] = true;
                continue;
            }
            singled[i] = true;
        } else if c.other_pos > 0 {
            c.score += 15;
            singled[i] = true;
        }
        if c.check_pos > 0 {
            c.score += CHECK_BONUS;
        }
    }
    let mut order: Vec<usize> = (0..cs.len()).collect();
    order.sort_by(|&a, &b| cs[b].score.cmp(&cs[a].score));
    let (mut first, mut later) = (Vec::new(), Vec::new());
    let mut seen: HashSet<String> = HashSet::new();
    for i in order {
        let c = cs[i].clone();
        if singled[i] || c.family.is_empty() || !seen.contains(&c.family) {
            seen.insert(c.family.clone());
            first.push(c);
        } else {
            seen.insert(c.family.clone());
            later.push(c);
        }
    }
    first.extend(later);
    *cs = first;
}

fn mentions(cs: &[Candidate], docs: &[String]) -> Vec<i64> {
    let mut by_form: HashMap<&str, usize> = HashMap::new();
    for (i, c) in cs.iter().enumerate() {
        for f in &c.forms {
            if !f.is_empty() {
                by_form.entry(f.as_str()).or_insert(i);
            }
        }
    }
    let mut first = vec![0i64; cs.len()];
    let mut mark = |key: &str, pos: usize| {
        if let Some(&i) = by_form.get(key) {
            if first[i] == 0 {
                first[i] = pos as i64 + 1;
            }
        }
    };
    for (pos, l) in docs.iter().take(400).enumerate() {
        let line = l.strip_prefix("./").unwrap_or(l);
        mark(&format!("={line}"), pos);
        let b = line.as_bytes();
        let (mut end, mut words) = (0usize, 0);
        while end < b.len() && words < 5 {
            match b[end + 1..].iter().position(|&c| c == b' ') {
                None => break,
                Some(n) => {
                    end += 1 + n;
                    mark(&line[..end], pos);
                }
            }
            words += 1;
        }
        mark(line, pos);
    }
    first
}

fn programs(cs: &[Candidate]) -> Vec<String> {
    let mut set: HashSet<String> = HashSet::new();
    for c in cs {
        for f in &c.forms {
            let f = f.strip_prefix('=').unwrap_or(f);
            let prog = f.split(' ').next().unwrap_or("");
            if !prog.is_empty() {
                set.insert(prog.to_string());
            }
        }
    }
    set.into_iter().collect()
}

const READMES: &[&str] = &["README.md", "readme.md", "Readme.md", "README.markdown", "README"];
const CONTRIBUTING: &[&str] = &["CONTRIBUTING.md", ".github/CONTRIBUTING.md", "docs/CONTRIBUTING.md"];
const AGENT_DOCS: &[&str] = &["AGENTS.md", "CLAUDE.md", "GEMINI.md", ".github/copilot-instructions.md", ".cursorrules"];

pub fn doc_commands(root: &str, progs: &[String]) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let (mut dev_lines, mut agent_lines, mut contrib_lines, mut rest_lines) = (vec![], vec![], vec![], vec![]);
    for s in code_snippets(&first_existing(root, READMES), progs) {
        if crate::pat::dev_heading(&s.1) {
            dev_lines.push(s.0);
        } else {
            rest_lines.push(s.0);
        }
    }
    for name in AGENT_DOCS {
        for s in code_snippets(&join(&[root, name]), progs) {
            agent_lines.push(s.0);
        }
    }
    for s in code_snippets(&first_existing(root, CONTRIBUTING), progs) {
        contrib_lines.push(s.0);
    }
    dev_lines.extend(naming(&setup_commands(root), progs));
    agent_lines.extend(naming(&check_commands(root), progs));
    (commands(&dev_lines), commands(&contrib_lines), commands(&rest_lines), commands(&agent_lines))
}

fn naming(lines: &[String], progs: &[String]) -> Vec<String> {
    lines.iter().filter(|l| progs.iter().any(|p| l.contains(p.as_str()))).map(|l| l.trim().to_string()).collect()
}

fn commands(snippets: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for s in snippets {
        let s = s.trim_start_matches(|c| "$%>❯➜".contains(c)).trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        for part in crate::pat::split_commands(s) {
            let part = fields(part).join(" ");
            if !part.is_empty() {
                out.push(part);
            }
        }
    }
    out
}

fn code_snippets(path: &str, progs: &[String]) -> Vec<(String, String)> {
    if path.is_empty() || progs.is_empty() {
        return Vec::new();
    }
    note(path);
    let f = match std::fs::File::open(crate::gofs::os(path)) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut data = Vec::new();
    let _ = f.take(512 << 10).read_to_end(&mut data);
    let names = |text: &str| progs.iter().any(|p| text.contains(p.as_str()));
    let mut out = Vec::new();
    let (mut fenced, mut heading) = (false, String::new());
    for raw in scan_lines(&data) {
        let lossy = crate::gofs::text(raw);
        let line = lossy.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
        } else if fenced {
            if names(line) {
                out.push((line.to_string(), heading.clone()));
            }
        } else if line.starts_with('#') {
            heading = line.to_string();
        } else if line.contains('`') {
            for m1 in crate::pat::inline_code(line) {
                if names(m1) {
                    out.push((m1.trim().to_string(), heading.clone()));
                }
            }
        }
    }
    out
}

pub fn scan_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut out: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
    if out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    for l in out.iter_mut() {
        if l.last() == Some(&b'\r') {
            *l = &l[..l.len() - 1];
        }
    }
    out
}

const MARKERS: &[&str] = &[
    "package.json",
    "justfile",
    "Justfile",
    ".justfile",
    "Makefile",
    "makefile",
    "GNUmakefile",
    "Taskfile.yml",
    "Taskfile.yaml",
    "taskfile.yml",
    "taskfile.yaml",
    "mise.toml",
    ".mise.toml",
    "deno.json",
    "deno.jsonc",
    "composer.json",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "compose.yaml",
    "compose.yml",
    "docker-compose.yml",
    "docker-compose.yaml",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "mix.exs",
    "Gemfile",
    ".git",
];

pub fn find_root(dir: &str) -> String {
    let home = home_dir().unwrap_or_default();
    let mut d = clean(dir);
    while d != home {
        for m in MARKERS {
            if exists(&join(&[&d, m])) && trusted(&d) {
                return d;
            }
        }
        let parent = dir_of(&d);
        if parent == d {
            break;
        }
        d = parent;
    }
    String::new()
}

pub fn ancestors(root: &str) -> Vec<String> {
    let home = home_dir().unwrap_or_default();
    let mut out = Vec::new();
    let mut d = root.to_string();
    while d != home {
        out.push(d.clone());
        let parent = dir_of(&d);
        if exists(&join(&[&d, ".git"])) || parent == d {
            break;
        }
        d = parent;
    }
    out
}

fn trusted(dir: &str) -> bool {
    match stat(dir) {
        Some(m) if m.mode() & 0o002 == 0 => m.uid() == uid() || m.uid() == 0,
        _ => false,
    }
}

fn project_clis(root: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(data) = read_file(&join(&[root, "package.json"])) {
        if let Some(doc) = json::parse(&data) {
            if let Some(m) = json::members(Some(&doc)) {
                if let Ok(name) = json::string_field(m, "name") {
                    let bin = json::raw(m, "bin");
                    if json::as_string(bin).is_some() && !name.is_empty() {
                        out.push(name[name.rfind('/').map_or(0, |i| i + 1)..].to_string());
                    }
                    for (k, _) in json::ordered(bin) {
                        out.push(k.clone());
                    }
                }
            }
        }
    }
    let d = join(&[root, "bin"]);
    note(&d);
    if let Ok(rd) = std::fs::read_dir(crate::gofs::os(&d)) {
        let mut names: Vec<(String, std::fs::DirEntry)> =
            rd.filter_map(|e| e.ok()).map(|e| (crate::gofs::os_text(&e.file_name()), e)).collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        for (n, e) in names {
            if let Ok(md) = e.metadata() {
                if is_regular(&md) && executable_bits(&md) {
                    out.push(format!("bin/{n}"));
                }
            }
        }
    }
    out
}

fn cli_commands(lines: &[String], clis: &[String]) -> Vec<Candidate> {
    if clis.is_empty() {
        return Vec::new();
    }
    let is_cli: HashSet<&str> = clis.iter().map(|s| s.as_str()).collect();
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for l in lines {
        let l = l.strip_prefix("./").unwrap_or(l);
        let words = fields(l);
        if words.is_empty() || !is_cli.contains(words[0].as_str()) || seen.contains(l) || out.len() == 12 {
            continue;
        }
        seen.insert(l.to_string());
        let mut name = words[0].as_str();
        for w in &words[1..] {
            if w != "run" && w != "exec" && !w.starts_with('-') {
                name = w;
                break;
            }
        }
        let c = task(l, "", name, &[]);
        if (c.known || c.stage != STAGE_OTHER) && name != words[0] {
            out.push(c);
        }
    }
    out
}
