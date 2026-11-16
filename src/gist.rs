use crate::suggest::{name_score, Candidate, STAGE_OTHER, STAGE_RISKY, STAGE_VERIFY};
use crate::{base_name, fields, one_line};

impl Candidate {
    pub fn running(mut self, body: &str) -> Candidate {
        let body = one_line(body);
        if body.is_empty() {
            return self;
        }
        if self.desc.is_empty() {
            let g = gist(&body);
            if g != self.cmd {
                self.desc = g;
            }
        }
        if !self.known && self.stage == STAGE_OTHER {
            if let Some(stage) = tool_stage(&body) {
                self.stage = stage;
                self.score = stage + 15;
                self.known = true;
            }
        }
        self
    }
}

fn tool_stages(key: &str) -> Option<i64> {
    Some(match key {
        "playwright" | "vitest" | "jest" | "mocha" | "cypress" | "ava" | "pytest" | "phpunit" | "rspec" | "go test"
        | "go vet" | "cargo test" | "cargo clippy" | "cargo fmt" | "tsc" | "eslint" | "prettier" | "biome"
        | "oxlint" | "oxfmt" | "stylelint" | "knip" | "ruff" | "mypy" | "golangci-lint" | "shellcheck" => STAGE_VERIFY,
        "playwright install" | "cypress install" | "cypress open" => 0,
        "changeset publish" | "npm publish" | "pnpm publish" | "yarn publish" | "cargo publish" | "docker push"
        | "git push" => STAGE_RISKY,
        _ => return None,
    })
}

pub fn tool_stage(body: &str) -> Option<i64> {
    let segs = segments(body);
    let last = segs.last()?;
    let w = bare(fields(last));
    if w.is_empty() {
        return None;
    }
    let tool = base_name(&w[0]);
    let sub = w[1..].iter().find(|x| !x.starts_with('-')).cloned().unwrap_or_default();
    if let Some(stage) = tool_stages(&format!("{tool} {sub}")) {
        return if stage != 0 { Some(stage) } else { None };
    }
    match tool.as_str() {
        "turbo" | "nx" | "lerna" | "npm" | "pnpm" | "yarn" | "bun" => {
            let mut rest = &w[1..];
            while !rest.is_empty() && matches!(rest[0].as_str(), "run" | "run-many" | "-t" | "--target") {
                rest = &rest[1..];
            }
            if !rest.is_empty() {
                let n = rest[0].strip_prefix("--target=").unwrap_or(&rest[0]);
                let (stage, _, known) = name_score(n);
                if known && stage == STAGE_VERIFY {
                    return Some(stage);
                }
            }
            None
        }
        _ => tool_stages(&tool),
    }
}

pub fn gist(body: &str) -> String {
    let mut segs = segments(body);
    for seg in &segs {
        if seg.matches('(').count() != seg.matches(')').count()
            || seg.matches('"').count() % 2 != 0
            || seg.matches('\'').count() % 2 != 0
        {
            return short(&bare(fields(&segs[0]))) + " …";
        }
    }
    let mut where_ = String::new();
    if segs.len() > 1 && segs[0].starts_with("cd ") {
        let d = segs[0][3..].trim();
        let d = d.strip_prefix("./").unwrap_or(d);
        let d = d.strip_suffix('/').unwrap_or(d);
        where_ = format!("{d}: ");
        segs.remove(0);
        let more = segs.iter().filter(|x| x.starts_with("cd ")).count();
        if more > 0 {
            where_ = format!("{} dirs: ", more + 1);
        }
    }
    let mut cmds: Vec<Vec<String>> = Vec::new();
    for seg in &segs {
        let w = bare(fields(seg));
        if !w.is_empty() && !(segs.len() > 1 && matches!(w[0].as_str(), "echo" | "printf" | "true" | "exit" | "cd")) {
            cmds.push(w);
        }
    }
    let mut parts: Vec<String> = Vec::new();
    let n = cmds.len();
    for w in cmds {
        let (pkg, script) = elsewhere(&w);
        let p = if !script.is_empty() && (!pkg.is_empty() || n > 1) {
            if pkg.is_empty() {
                script
            } else {
                format!("{pkg}: {script}")
            }
        } else {
            short(&w)
        };
        if !p.is_empty() && !parts.contains(&p) {
            parts.push(p);
        }
    }
    if parts.len() > 3 {
        let more = parts.len() - 2;
        parts.truncate(2);
        parts.push(format!("+{more}"));
    }
    where_ + &parts.join(", ")
}

fn short(w: &[String]) -> String {
    let Some(first) = w.first() else { return String::new() };
    let tool = base_name(first.trim_matches(['"', '\'']));
    let args = &w[1..];
    match tool.as_str() {
        "concurrently" | "run-p" | "run-s" | "npm-run-all" => {
            let joined = args.join(" ");
            let mut names: Vec<String> = joined
                .split(['"', '\''])
                .skip(1)
                .step_by(2)
                .map(|q| short(&bare(fields(q))))
                .filter(|q| !q.is_empty())
                .collect();
            if names.is_empty() {
                names = args.iter().filter(|a| !a.starts_with('-')).cloned().collect();
            }
            if !names.is_empty() {
                return names.join(", ");
            }
        }
        "node" | "nodejs" | "bun" | "deno" | "tsx" | "ts-node" | "python" | "python3" | "ruby" | "perl" | "php"
        | "sh" | "bash" | "zsh" => {
            if let Some(i) = args.iter().position(|a| a == "-m") {
                if let Some(m) = args.get(i + 1) {
                    return m.clone();
                }
            }
            let files: Vec<&str> = args
                .iter()
                .map(|a| a.trim_matches(['"', '\'']))
                .filter(|a| !a.starts_with('-') && !a.contains('*') && !a.starts_with('$'))
                .collect();
            let ext = |a: &&&str| base_name(a).trim_start_matches('.').contains('.');
            if let Some(f) = files.iter().find(ext).or_else(|| files.iter().find(|a| a.contains('/'))) {
                return base_name(f);
            }
            if let Some(flag) = args.first().filter(|a| a.starts_with("--") && !a.contains('=')) {
                return format!("{tool} {flag}");
            }
        }
        _ => {}
    }
    let plain = |a: &str| {
        a.starts_with(|c: char| c.is_ascii_alphabetic())
            && a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-'))
    };
    let mut out = vec![tool];
    for a in args {
        if out.len() == 3 || !plain(a) {
            break;
        }
        out.push(a.clone());
    }
    out.join(" ")
}

pub fn segments(body: &str) -> Vec<String> {
    crate::pat::split_commands(&one_line(body))
        .into_iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub fn bare(mut w: Vec<String>) -> Vec<String> {
    while w.len() > 1 {
        let x = w[0].clone();
        if crate::pat::env_assign(&x) {
            for q in ['"', '\''] {
                while w[0].matches(q).count() % 2 != 0 && w.len() > 2 {
                    let next = w.remove(1);
                    w[0] = format!("{} {}", w[0], next);
                }
            }
            w.remove(0);
        } else if matches!(x.as_str(), "cross-env" | "env" | "npx" | "bunx" | "pnpx" | "time") || x.starts_with('-') {
            w.remove(0);
        } else if matches!(x.as_str(), "pnpm" | "yarn" | "npm" | "bun" | "nx")
            && matches!(w[1].as_str(), "exec" | "dlx" | "x")
        {
            w.drain(..2);
        } else if x == "dotenv" || x == "dotenvx" {
            match w.iter().position(|y| y == "--") {
                Some(cut) if cut + 1 < w.len() => {
                    w.drain(..cut + 1);
                }
                _ => return w,
            }
        } else {
            return w;
        }
    }
    w
}

pub fn elsewhere(w: &[String]) -> (String, String) {
    if !matches!(w[0].as_str(), "npm" | "pnpm" | "yarn" | "bun") {
        return (String::new(), String::new());
    }
    let (mut pkg, mut script) = (String::new(), String::new());
    let mut i = 1;
    while i < w.len() {
        let x = &w[i];
        let (flag, mut value, joined) = match x.split_once('=') {
            Some((f, v)) => (f, v.to_string(), true),
            None => (x.as_str(), String::new(), false),
        };
        if matches!(flag, "--filter" | "-F" | "-C" | "--dir" | "--cwd" | "--prefix" | "-w" | "--workspace")
            || x == "workspace"
        {
            if !joined && i + 1 < w.len() {
                i += 1;
                value = w[i].clone();
            }
            if pkg.is_empty() {
                let v = value.strip_prefix("./").unwrap_or(&value);
                pkg = v.trim_matches(|c| "\"'{}".contains(c)).to_string();
            }
        } else if x == "run" || x == "run-script" || x.starts_with('-') {
        } else if script.is_empty() {
            script = x.clone();
        }
        i += 1;
    }
    (pkg, script)
}
