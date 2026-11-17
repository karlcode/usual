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
            let first = describe(&bare(fields(&segs[0])), false);
            return if first.is_empty() { first } else { first + " …" };
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
        if !w.is_empty() && !(segs.len() > 1 && STEP_NOISE.contains(&base_name(&w[0]).as_str())) {
            cmds.push(w);
        }
    }
    let mut parts: Vec<String> = Vec::new();
    let n = cmds.len();
    for w in cmds {
        let p = describe(&w, n > 1);
        let key = |s: &str| s.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "");
        if !p.is_empty() && !parts.iter().any(|q| key(q) == key(&p)) {
            parts.push(p);
        }
    }
    let list = listing(parts);
    if list.is_empty() {
        return list;
    }
    where_ + &list
}

/// Several names as one hint: a prefix they share said once (test:astro, test:e2e → test: astro, e2e;
/// compile-client, compile-copilot → compile: client, copilot), and past three, a count.
fn listing(mut parts: Vec<String>) -> String {
    if parts.len() > 1 {
        for sep in [':', '-'] {
            let Some((head, _)) = parts[0].split_once(sep) else { continue };
            let pre = format!("{head}{sep}");
            // compile-client, compile-copilot share a word; @a/web-app, @a/web-docs share half a package name
            let names = sep == ':' || parts.iter().all(|p| !p.contains([' ', ':', '/', '@']));
            if names
                && !head.is_empty()
                && !head.contains(' ')
                && parts.iter().all(|p| p.starts_with(&pre) && p.len() > pre.len())
            {
                let rest: Vec<String> = parts.iter().map(|p| p[pre.len()..].trim().to_string()).collect();
                parts = vec![format!("{head}: {}", listing(rest))];
                break;
            }
        }
    }
    if parts.len() > 3 {
        let more = parts.len() - 2;
        parts.truncate(2);
        parts.push(format!("+{more}"));
    }
    parts.join(", ")
}

// Steps that set up the next one and say nothing about the task.
const STEP_NOISE: &[&str] = &[
    "echo", "printf", "true", "exit", "cd", "mkdir", "sleep", "touch", "set", "export", ":", "if", "fi", "for", "done",
    "while", "case", "esac", "[", "[[", "test", "wait-on",
];

// A word that names only the runner says nothing: `pnpm`, or `cargo` from `cargo +nightly …`.
const RUNNERS: &[&str] = &[
    "npm", "pnpm", "pn", "yarn", "bun", "npx", "bunx", "deno", "node", "cargo", "go", "git", "sh", "bash", "zsh",
    "env", "python", "python3", "uv", "poetry", "pip", "docker", "turbo", "nx", "lerna", "mise",
];

/// One command of a script, in words: a package's script, a step in plain words when it is one of a chain,
/// else the tool and its subcommand. Empty when all that is left is noise.
fn describe(w: &[String], chained: bool) -> String {
    if w.is_empty() {
        return String::new();
    }
    let (pkg, script) = elsewhere(w);
    let p = if !script.is_empty() && (!pkg.is_empty() || chained) {
        if pkg.is_empty() {
            script
        } else {
            format!("{pkg}: {script}")
        }
    } else if let Some(p) = fan_out(w) {
        p
    } else if let Some(p) = chained.then(|| plain(w)).flatten() {
        p.to_string()
    } else {
        short(w)
    };
    let words: Vec<&str> = p.split(' ').collect();
    let junk = p.is_empty()
        || p.starts_with(['-', '$', '(', '\'', '"', '+', '{', '<', '>', '|', '\\'])
        || p.ends_with(':')
        || p.contains(';')
        || p.contains("$(")
        || p.contains("${")
        || p.contains("$$")
        || words[0].contains('=')
        || (words.len() == 1 && RUNNERS.contains(&words[0]))
        || (words.len() == 2 && RUNNERS.contains(&words[0]) && matches!(words[1], "run" | "exec" | "x"));
    if junk {
        String::new()
    } else {
        p
    }
}

/// A runner that does one task in many packages: the task, and where.
fn fan_out(w: &[String]) -> Option<String> {
    let tool = base_name(&w[0]);
    let args = &w[1..];
    let value = |names: &[&str]| -> Option<String> {
        for (i, a) in args.iter().enumerate() {
            for n in names {
                if let Some(v) = a.strip_prefix(&format!("{n}=")) {
                    return Some(v.to_string());
                }
                if a == n {
                    return args.get(i + 1).cloned();
                }
            }
        }
        None
    };
    let positional: Vec<&String> = {
        let mut out = Vec::new();
        let mut skip = false;
        for a in args {
            if skip {
                skip = false;
            } else if a.starts_with('-') {
                skip = !a.contains('=')
                    && matches!(
                        a.as_str(),
                        "--filter" | "-F" | "-t" | "--target" | "--targets" | "-p" | "--projects" | "--scope"
                    );
            } else {
                out.push(a);
            }
        }
        out
    };
    // --filter=web, --filter=web... (and its dependencies), --filter='!docs' (all but), --filter='./packages/*'
    let unquote = |s: String| {
        let s = s.trim_matches(|c| "\"'{}".contains(c)).trim_start_matches("./").trim_matches('.').to_string();
        if s.starts_with('!') || s.contains('*') {
            "every package".to_string()
        } else {
            s
        }
    };
    match tool.as_str() {
        "turbo" => {
            let task = positional.iter().find(|a| a.as_str() != "run")?;
            let pkg = value(&["--filter", "-F"]).map(unquote).unwrap_or_else(|| "every package".into());
            Some(format!("{pkg}: {task}"))
        }
        "nx" => match positional.first()?.as_str() {
            "run-many" | "affected" => {
                let task = value(&["-t", "--target", "--targets"])?.replace(',', ", ");
                let pkg = value(&["-p", "--projects"]).map(unquote).unwrap_or_else(|| {
                    if positional[0] == "affected" {
                        "affected projects".into()
                    } else {
                        "every project".into()
                    }
                });
                Some(format!("{pkg}: {task}"))
            }
            "run" => {
                let (proj, task) = positional.get(1)?.split_once(':')?;
                Some(format!("{proj}: {task}"))
            }
            _ => None,
        },
        "lerna" if positional.first().map(|s| s.as_str()) == Some("run") => {
            let task = positional.get(1)?;
            let pkg = value(&["--scope"]).map(unquote).unwrap_or_else(|| "every package".into());
            Some(format!("{pkg}: {task}"))
        }
        _ => None,
    }
}

/// What a well-known step does, in a word or two. In a chain, `type-check, lint, build` reads at a glance
/// where `tsc, eslint, vite build` has to be decoded; alone, the tool's name is the more useful hint.
fn plain(w: &[String]) -> Option<&'static str> {
    let tool = base_name(w.first()?.trim_matches(['"', '\'']));
    let args = &w[1..];
    let sub = args.iter().find(|a| !a.starts_with('-') && !a.starts_with('+')).map(String::as_str).unwrap_or("");
    let has = |f: &str| args.iter().any(|a| a == f || (a.starts_with(f) && a[f.len()..].starts_with('=')));
    let check = has("--check") || has("-c") || has("--list-different") || has("-l");
    Some(match (tool.as_str(), sub) {
        ("rm" | "rimraf" | "del" | "del-cli", _) | ("git" | "cargo" | "go", "clean") => "clean",
        ("tsc" | "tsgo" | "vue-tsc" | "svelte-check" | "mypy" | "pyright" | "flow", _)
        | ("astro" | "cargo", "check") => "type-check",
        ("vitest", "bench") | ("cargo", "bench") => "benchmarks",
        ("vitest" | "jest" | "mocha" | "ava" | "tap" | "uvu" | "pytest" | "phpunit" | "rspec", _)
        | ("bun" | "deno" | "go" | "cargo" | "mix", "test")
        | ("cargo", "nextest") => "tests",
        ("node", _) if has("--test") => "tests",
        ("playwright", "test") | ("cypress", "run") | ("wdio", _) => "browser tests",
        ("playwright", "install") => "browsers",
        ("eslint" | "oxlint" | "xo" | "standard" | "golangci-lint" | "shellcheck" | "stylelint", _)
        | ("biome" | "next", "lint")
        | ("cargo", "clippy")
        | ("go", "vet")
        | ("ruff", "check") => "lint",
        ("prettier" | "oxfmt" | "dprint" | "black" | "gofmt", _) | ("biome" | "ruff" | "cargo", "format" | "fmt")
            if check =>
        {
            "format check"
        }
        ("prettier" | "oxfmt" | "dprint" | "black" | "gofmt", _) | ("biome" | "ruff", "format") | ("cargo", "fmt") => {
            "format"
        }
        ("knip", _) => "unused code",
        ("vite", "" | "dev" | "serve")
        | ("next" | "nuxt" | "nuxi" | "astro" | "remix" | "vinxi" | "react-router" | "vitepress", "dev")
        | ("react-scripts", "start")
        | ("webpack", "serve")
        | ("webpack-dev-server", _)
        | ("vue-cli-service" | "ng", "serve") => "dev server",
        (
            "vite" | "next" | "nuxt" | "nuxi" | "astro" | "remix" | "vinxi" | "react-router" | "react-scripts",
            "build",
        )
        | ("webpack", "" | "build")
        | ("rollup" | "tsup" | "tsdown" | "esbuild" | "unbuild" | "rolldown", _)
        | ("parcel" | "vue-cli-service" | "ng" | "cargo" | "go" | "bun", "build")
        | ("nuxt" | "nuxi", "generate") => "build",
        ("vite" | "nuxt" | "nuxi" | "astro", "preview") | ("next", "start") => "serve the build",
        ("storybook", "dev") | ("start-storybook", _) => "Storybook",
        ("storybook", "build") | ("build-storybook", _) => "build Storybook",
        ("vitepress", "build") => "build the docs",
        ("typedoc", _) => "API docs",
        ("tailwindcss" | "postcss" | "sass", _) => "CSS",
        ("npm" | "pnpm" | "yarn" | "bun", "install" | "i" | "ci")
        | ("uv", "sync")
        | ("pip" | "poetry" | "bundle", "install") => "install",
        ("husky" | "simple-git-hooks", _) | ("lefthook", "install") => "git hooks",
        ("go" | "buf", "generate") | ("graphql-codegen", _) => "generate code",
        ("prisma", "generate") => "Prisma client",
        ("prisma", "migrate") | ("drizzle-kit", "migrate" | "push") => "migrate the database",
        ("docker" | "docker-compose" | "podman", _) if has("up") || args.iter().any(|a| a == "up") => "containers up",
        ("docker" | "docker-compose" | "podman", _) if args.iter().any(|a| a == "down") => "containers down",
        ("docker", "build") => "image",
        ("nodemon", _) | ("tsx", "watch") => "restart on change",
        ("cp" | "cpy" | "cpx" | "copyfiles", _) => "copy files",
        _ => return None,
    })
}

// Flags that change what a task does, kept where the rest are dropped: `tsc --watch` and `tsc` are two tasks.
const TELLING_FLAGS: &[&str] = &["--watch", "--fix", "--coverage", "--check", "--ui"];

/// A script file's name in words when it has several: build-all-release-channels.js → build all release channels.
fn humanize(file: &str) -> String {
    let name = base_name(file);
    let stem = match name.rsplit_once('.') {
        Some((s, ext)) if !s.is_empty() && ext.len() <= 4 => s,
        _ => return name,
    };
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = stem.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let boundary = c.is_ascii_uppercase()
            && i > 0
            && (chars[i - 1].is_ascii_lowercase()
                || chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()) && chars[i - 1].is_ascii_uppercase());
        if matches!(c, '-' | '_' | '.' | ' ') || boundary {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            if boundary {
                cur.push(c);
            }
            continue;
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    if words.len() < 2 || words.iter().any(|w| !w.chars().all(|c| c.is_ascii_alphanumeric())) {
        return name;
    }
    words
        .iter()
        .map(|w| {
            if w.len() > 1 && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
                w.clone()
            } else {
                w.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn short(w: &[String]) -> String {
    let Some(first) = w.first() else { return String::new() };
    let tool = base_name(first.trim_matches(['"', '\'']));
    let args = &w[1..];
    match tool.as_str() {
        "concurrently" | "run-p" | "run-s" | "npm-run-all" | "npm-run-all2" => {
            // concurrently -n api,web names its processes; those names are the hint.
            if let Some(i) = args.iter().position(|a| a == "-n" || a == "--names") {
                if let Some(names) = args.get(i + 1) {
                    let names: Vec<String> = names
                        .trim_matches(['"', '\''])
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                    if !names.is_empty() && !names.iter().any(|n| n.ends_with(':')) {
                        return listing(names);
                    }
                }
            }
            if let Some(names) = args.iter().find_map(|a| a.strip_prefix("--names=")) {
                return names.trim_matches(['"', '\'']).split(',').collect::<Vec<_>>().join(", ");
            }
            let joined = args.join(" ");
            let mut names: Vec<String> = joined
                .split(['"', '\''])
                .skip(1)
                .step_by(2)
                .map(|q| {
                    // "npm:lint:*" is concurrently's shorthand for scripts by name
                    match q.split_once(':') {
                        Some(("npm" | "pnpm" | "yarn" | "bun", script)) => {
                            script.split('(').next().unwrap_or(script).to_string()
                        }
                        // side by side, the commands read as steps do
                        _ if segments(q).len() == 1 => describe(&bare(fields(q)), true),
                        _ => gist(q),
                    }
                })
                .filter(|q| !q.is_empty())
                .collect();
            if names.is_empty() {
                names = args.iter().filter(|a| !a.starts_with('-') && !a.contains(['"', '\''])).cloned().collect();
            }
            names.dedup();
            if !names.is_empty() {
                return listing(names);
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
                return humanize(f);
            }
            if let Some(flag) = args.first().filter(|a| a.starts_with("--") && !a.contains('=')) {
                return format!("{tool} {flag}");
            }
        }
        _ => {}
    }
    let plain_word = |a: &str| {
        a.starts_with(|c: char| c.is_ascii_alphabetic())
            && a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-'))
    };
    let file = first.contains('/') || tool.contains('.');
    let has_words = args.first().is_some_and(|a| plain_word(a));
    let mut out = vec![if file && !has_words { humanize(&tool) } else { tool }];
    let mut words = true;
    for a in args {
        if a.starts_with('+') && out.len() == 1 && words {
            continue; // cargo +nightly fmt
        }
        if a.starts_with('-') {
            words = false;
            if TELLING_FLAGS.contains(&a.as_str()) && !out.contains(a) {
                out.push(a.clone());
            }
            continue;
        }
        if !words || out.len() == 3 || !plain_word(a) {
            words = false;
            continue;
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
        } else if x == "env" && w.len() > 3 && w[1] == "-u" {
            w.drain(..3); // env -u TMUX cmd: unsets a name, runs cmd
        } else if matches!(x.as_str(), "cross-env" | "env" | "npx" | "bunx" | "pnpx" | "time") || x.starts_with('-') {
            w.remove(0);
        } else if matches!(x.as_str(), "pnpm" | "yarn" | "npm" | "bun" | "nx")
            && matches!(w[1].as_str(), "exec" | "dlx" | "x")
        {
            w.drain(..2);
        } else if matches!(x.as_str(), "pnpm" | "yarn" | "bun") && KNOWN_CLIS.contains(&w[1].as_str()) {
            w.remove(0); // yarn concurrently …, pnpm tsc --build: the package manager only finds the tool
        } else if x.starts_with('+') || matches!(x.as_str(), "then" | "do" | "else" | "elif") {
            w.remove(0); // cargo +nightly …; `then cmake …` is cmake
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

// Tools a package manager is often asked to find (`yarn concurrently`, `pnpm tsc`), as opposed to scripts.
const KNOWN_CLIS: &[&str] = &[
    "concurrently",
    "run-p",
    "run-s",
    "npm-run-all",
    "npm-run-all2",
    "turbo",
    "nx",
    "lerna",
    "tsc",
    "tsgo",
    "vue-tsc",
    "svelte-check",
    "vitest",
    "jest",
    "mocha",
    "playwright",
    "eslint",
    "oxlint",
    "prettier",
    "oxfmt",
    "biome",
    "knip",
    "vite",
    "next",
    "nuxt",
    "nuxi",
    "astro",
    "tsx",
    "tsup",
    "tsdown",
    "rimraf",
    "changeset",
    "prisma",
    "drizzle-kit",
    "storybook",
    "typedoc",
    "webpack",
    "rollup",
    "esbuild",
    "tailwindcss",
    "cross-env",
    "dotenv",
    "env",
];

pub fn elsewhere(w: &[String]) -> (String, String) {
    if !matches!(w[0].as_str(), "npm" | "pnpm" | "yarn" | "bun") {
        return (String::new(), String::new());
    }
    let (mut pkg, mut script) = (String::new(), String::new());
    let every = "every package".to_string();
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
                let v = v.trim_matches(|c| "\"'{}".contains(c));
                // packages/*, ./packages/**, '*': not a package, all of them
                pkg = if v.contains('*') { every.clone() } else { v.to_string() };
            }
        } else if matches!(x.as_str(), "-r" | "--recursive" | "--workspaces" | "-ws") {
            if pkg.is_empty() {
                pkg = every.clone();
            }
        } else if x == "workspaces" && w.get(i + 1).is_some_and(|f| f == "foreach") {
            pkg = every.clone();
            i += 1;
        } else if x == "run" || x == "run-script" || x.starts_with('-') {
        } else if script.is_empty() {
            if KNOWN_CLIS.contains(&x.as_str()) {
                // yarn --cwd app cross-env X=1 vite build: a command, run in that package
                let inner = bare(w[i..].to_vec());
                script = plain(&inner).map(String::from).unwrap_or_else(|| short(&inner));
                break;
            }
            script = x.clone();
        }
        i += 1;
    }
    (pkg, script)
}
