use crate::fields;
use crate::gofs::{base, exists, first_existing, glob, is_regular, join, note, stat};
use crate::json::{self, J};
use crate::suggest::{ancestors, setup, task, Candidate};
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

type Source = fn(&str) -> Vec<Candidate>;

pub static SOURCES: &[(&str, Source)] = &[
    ("submodules", submodule_setup),
    ("node", node_tasks),
    ("just", just_tasks),
    ("taskfile", taskfile_tasks),
    ("mise", mise_tasks),
    ("make", make_tasks),
    ("deno", deno_tasks),
    ("composer", composer_tasks),
    ("cargo", cargo_tasks),
    ("go", go_tasks),
    ("python", python_tasks),
    ("jvm", jvm_tasks),
    ("mix", mix_tasks),
    ("scripts", conventional_scripts),
    ("compose", compose_tasks),
    ("env", env_setup),
];

const MAX_FILE: u64 = 1 << 20;

pub fn read_file(path: &str) -> Option<Vec<u8>> {
    note(path);
    let md = stat(path)?;
    if !is_regular(&md) || md.len() > MAX_FILE {
        return None;
    }
    std::fs::read(crate::gofs::os(path)).ok()
}

pub fn lines(path: &str) -> Vec<String> {
    match read_file(path) {
        None => Vec::new(),
        Some(d) => crate::gofs::text(&d).replace("\r\n", "\n").split('\n').map(String::from).collect(),
    }
}

fn contains(hay: &Option<Vec<u8>>, needle: &[u8]) -> bool {
    hay.as_ref().is_some_and(|h| h.windows(needle.len()).any(|w| w == needle))
}

fn mtime(path: &str) -> Option<SystemTime> {
    note(path);
    stat(path)?.modified().ok()
}

fn stale_since(lock: &str, installed: &str) -> bool {
    let (l, i) = (mtime(lock), mtime(installed));
    match (l, i) {
        (Some(l), Some(i)) => l > i + std::time::Duration::from_secs(1),
        _ => false,
    }
}

fn trim_quotes(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

fn env_setup(root: &str) -> Vec<Candidate> {
    if exists(&join(&[root, ".env"])) {
        return vec![];
    }
    let ex = first_existing(root, &[".env.example", ".env.sample", ".env.template", ".env.dist"]);
    if !ex.is_empty() {
        let mut c = setup(&format!("cp {} .env", base(&ex)));
        c.desc = "no .env yet".into();
        return vec![c];
    }
    vec![]
}

fn submodule_setup(root: &str) -> Vec<Candidate> {
    if !exists(&join(&[root, ".gitmodules"])) {
        return vec![];
    }
    let mut docs =
        read_file(&first_existing(root, &["README.md", "readme.md", "Readme.md", "README"])).unwrap_or_default();
    docs.extend(
        read_file(&first_existing(root, &["CONTRIBUTING.md", ".github/CONTRIBUTING.md", "docs/CONTRIBUTING.md"]))
            .unwrap_or_default(),
    );
    if !String::from_utf8_lossy(&docs).to_lowercase().contains("submodule") {
        return vec![];
    }
    for l in lines(&join(&[root, ".gitmodules"])) {
        let Some((k, v)) = l.trim().split_once('=') else { continue };
        if k.trim() != "path" {
            continue;
        }
        let sub = join(&[root, v.trim()]);
        note(&sub);
        let empty_or_missing = match std::fs::read_dir(crate::gofs::os(&sub)) {
            Ok(mut rd) => rd.next().is_none(),
            Err(e) => e.raw_os_error() == Some(2),
        };
        if empty_or_missing {
            let mut c = setup("git submodule update --init --recursive");
            c.desc = "submodules not checked out".into();
            return vec![c];
        }
    }
    vec![]
}

fn npm_lifecycle(n: &str) -> bool {
    matches!(
        n,
        "preinstall"
            | "install"
            | "postinstall"
            | "preuninstall"
            | "uninstall"
            | "postuninstall"
            | "prepublish"
            | "preprepare"
            | "prepare"
            | "postprepare"
            | "prepublishOnly"
            | "prepack"
            | "postpack"
            | "publish"
            | "postpublish"
            | "preversion"
            | "version"
            | "postversion"
            | "dependencies"
    )
}

fn pm_builtin(n: &str) -> bool {
    matches!(
        n,
        "add"
            | "audit"
            | "bin"
            | "cache"
            | "config"
            | "create"
            | "dedupe"
            | "dlx"
            | "exec"
            | "help"
            | "import"
            | "info"
            | "init"
            | "install"
            | "link"
            | "list"
            | "ln"
            | "login"
            | "logout"
            | "ls"
            | "outdated"
            | "pack"
            | "patch"
            | "prune"
            | "publish"
            | "rebuild"
            | "remove"
            | "rm"
            | "root"
            | "run"
            | "server"
            | "setup"
            | "store"
            | "unlink"
            | "up"
            | "update"
            | "upgrade"
            | "version"
            | "why"
            | "workspace"
            | "workspaces"
    )
}

fn node_tasks(root: &str) -> Vec<Candidate> {
    let Some(data) = read_file(&join(&[root, "package.json"])) else { return vec![] };
    let Some(doc) = json::parse(&data) else { return vec![] };
    let Some(m) = json::members(Some(&doc)) else { return vec![] };
    let Ok(pm_field) = json::string_field(m, "packageManager") else { return vec![] };
    let (scripts, deps, dev_deps) =
        (json::raw(m, "scripts"), json::raw(m, "dependencies"), json::raw(m, "devDependencies"));
    let (pm, lock) = package_manager(root, &pm_field);

    let mut out = Vec::new();
    if pm != "deno" && (json::has_members(deps) || json::has_members(dev_deps)) {
        let installed = node_installed(root);
        if installed.is_empty() {
            out.push(setup(&format!("{pm} install")));
        } else if !lock.is_empty() && stale_since(&lock, &installed) {
            let mut c = setup(&format!("{pm} install"));
            c.desc = format!("{} changed since the last install", base(&lock));
            out.push(c);
        }
    }
    let scripts = json::ordered(scripts);
    let names: HashSet<&str> = scripts.iter().map(|(k, _)| k.as_str()).collect();
    for (name, val) in scripts {
        if npm_lifecycle(name)
            || name.starts_with('_')
            || name.starts_with("pre") && names.contains(&name[3..])
            || name.starts_with("post") && names.contains(&name[4..])
        {
            continue;
        }
        let body = json::as_string(Some(val)).unwrap_or_default();
        if placeholder(&body) {
            continue;
        }
        let forms = npm_forms(name);
        let forms: Vec<&str> = forms.iter().map(|s| s.as_str()).collect();
        out.push(task(&npm_command(&pm, name), "", name, &forms).running(&body));
    }
    out
}

fn package_manager(root: &str, field: &str) -> (String, String) {
    const LOCKS: &[(&str, &str)] = &[
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("package-lock.json", "npm"),
        ("npm-shrinkwrap.json", "npm"),
    ];
    let field = field.split('@').next().unwrap_or("");
    for d in ancestors(root) {
        for (file, pm) in LOCKS {
            let p = join(&[&d, file]);
            if exists(&p) && (field.is_empty() || field == *pm) {
                return (pm.to_string(), p);
            }
        }
    }
    match field {
        "npm" | "pnpm" | "yarn" | "bun" | "deno" => (field.to_string(), String::new()),
        _ => ("npm".into(), String::new()),
    }
}

fn node_installed(root: &str) -> String {
    for d in ancestors(root) {
        let p = join(&[&d, ".pnp.cjs"]);
        if exists(&p) {
            return p;
        }
        let nm = join(&[&d, "node_modules"]);
        if !exists(&nm) {
            continue;
        }
        for marker in [".package-lock.json", ".modules.yaml", ".yarn-state.yml", ".yarn-integrity"] {
            let p = join(&[&nm, marker]);
            if exists(&p) {
                return p;
            }
        }
        return nm;
    }
    String::new()
}

fn placeholder(body: &str) -> bool {
    let b = body.trim();
    b.contains("no test specified")
        || (b.starts_with("echo") || b.starts_with("node -e"))
            && (b.ends_with("exit 1") || b.contains("exit(1)") || b.contains("console.error"))
}

fn npm_command(pm: &str, name: &str) -> String {
    if pm == "npm" && matches!(name, "start" | "test" | "stop" | "restart") {
        format!("npm {name}")
    } else if pm == "deno" {
        format!("deno task {name}")
    } else if pm == "npm" || pm == "bun" || pm_builtin(name) {
        format!("{pm} run {name}")
    } else {
        format!("{pm} {name}")
    }
}

fn npm_forms(name: &str) -> Vec<String> {
    let mut f: Vec<String> =
        ["npm run ", "npm run-script ", "yarn ", "yarn run ", "pnpm ", "pnpm run ", "bun run ", "bun "]
            .iter()
            .map(|p| format!("{p}{name}"))
            .collect();
    if name == "start" || name == "test" {
        f.push(format!("npm {name}"));
    }
    f
}

fn just_tasks(root: &str) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let (mut doc, mut private, mut first, mut open) = (String::new(), false, true, false);
    for l in lines(&first_existing(root, &["justfile", "Justfile", ".justfile"])) {
        if l.starts_with(' ') || l.starts_with('\t') {
            let t = l.trim().trim_start_matches(['@', '-']);
            if open && !t.is_empty() && !t.starts_with('#') {
                let last = out.pop().unwrap();
                out.push(last.running(t));
                open = false;
            }
            continue;
        }
        open = false;
        if l.starts_with('#') && !l.starts_with("#!") {
            // A comment can run over several lines; `just --list` shows only the last, often mid-sentence.
            let t = l.trim_start_matches('#').trim();
            let divider = ["---", "===", "***", "~~~", "///", "###"].iter().any(|d| t.starts_with(d));
            if !divider && t.chars().any(char::is_alphanumeric) {
                if !doc.is_empty() {
                    doc.push(' ');
                }
                doc.push_str(t);
            } else {
                doc.clear(); // a divider: what follows is a new block
            }
            continue;
        }
        if l.starts_with('[') {
            private = private || l.contains("private");
            if let Some(d) = crate::pat::just_doc(&l) {
                doc = d.to_string();
            }
            continue;
        }
        if let Some((name, after)) = crate::pat::just_recipe(&l) {
            if !after.starts_with('=') {
                match name {
                    "set" | "export" | "alias" | "import" | "mod" | "if" | "else" => {}
                    _ => {
                        if !private && !name.starts_with('_') {
                            let cmd = format!("just {name}");
                            let forms: Vec<&str> = if first { vec![cmd.as_str(), "=just"] } else { vec![cmd.as_str()] };
                            out.push(task(&cmd, &crate::first_sentence(&doc), name, &forms));
                            open = true;
                        }
                        first = false;
                    }
                }
            }
        }
        doc.clear();
        private = false;
    }
    out
}

fn make_tasks(root: &str) -> Vec<Candidate> {
    struct Target {
        name: String,
        desc: String,
        recipe: String,
        needs: String,
    }
    let mut targets: Vec<Target> = Vec::new();
    let mut phony: HashSet<String> = HashSet::new();
    let mut vars: HashMap<String, String> = HashMap::new();
    let (mut prev_doc, mut open) = (String::new(), 0usize);
    for l in lines(&first_existing(root, &["GNUmakefile", "Makefile", "makefile"])) {
        let mut doc = std::mem::take(&mut prev_doc);
        if l.starts_with("##") {
            if !l.starts_with("##@") {
                let t = l.trim_start_matches('#').trim();
                prev_doc = if doc.is_empty() { t.to_string() } else { format!("{doc} {t}") };
            }
            continue;
        }
        if l.starts_with('\t') {
            let t = l.trim().trim_start_matches(['@', '-', '+']);
            if open > 0 && !t.is_empty() && !crate::pat::recipe_noise(t) {
                let t = t.strip_suffix('\\').unwrap_or(t);
                let t = crate::pat::replace_make_vars(t, |r| {
                    vars.get(r.trim_matches(|c| "${}()".contains(c))).cloned().unwrap_or_else(|| r.to_string())
                });
                let n = targets.len();
                for tg in &mut targets[n - open..] {
                    tg.recipe = t.clone();
                }
                open = 0;
            }
            continue;
        }
        if l.is_empty() || l.starts_with(' ') || l.starts_with('#') {
            continue;
        }
        open = 0;
        if let Some((name, v)) = crate::pat::make_assign(&l) {
            let v = v.trim();
            if !v.is_empty() && !v.contains(['$', '#']) {
                vars.insert(name.to_string(), v.to_string());
            }
            continue;
        }
        let colon = match l.find(':') {
            Some(c) if c > 0 => c,
            _ => continue,
        };
        if l[..colon].contains(['=', '$', '(']) || l[colon..].starts_with(":=") || l[colon..].starts_with("::=") {
            continue;
        }
        let rest = &l[colon + 1..];
        if l.starts_with(".PHONY") {
            for n in fields(rest) {
                phony.insert(n);
            }
            continue;
        }
        if let Some((_, d)) = rest.split_once("##") {
            doc = d.trim().to_string();
        }
        let mut needs: Vec<String> = Vec::new();
        for n in fields(rest.split('#').next().unwrap_or("")) {
            if rest.contains('=') {
                break;
            }
            if !n.contains(['$', '%', '|', ';']) && needs.len() < 4 {
                needs.push(n);
            }
        }
        for n in fields(&l[..colon]) {
            if !n.contains(['%', '.', '/', '\\']) {
                targets.push(Target {
                    name: n,
                    desc: crate::first_sentence(&doc),
                    recipe: String::new(),
                    needs: needs.join(", "),
                });
                open += 1;
            }
        }
    }
    let mut out = Vec::new();
    for (i, t) in targets.iter().enumerate() {
        if !phony.is_empty() && !phony.contains(&t.name) && t.desc.is_empty() {
            continue;
        }
        let cmd = format!("make {}", t.name);
        let forms: Vec<&str> = if i == 0 { vec![cmd.as_str(), "=make"] } else { vec![cmd.as_str()] };
        let mut c = task(&cmd, &t.desc, &t.name, &forms).running(&t.recipe);
        if c.desc.is_empty() {
            c.desc = t.needs.clone();
        }
        out.push(c);
    }
    out
}

fn taskfile_tasks(root: &str) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut internal: HashSet<usize> = HashSet::new();
    let mut bodies: HashMap<usize, String> = HashMap::new();
    let (mut inside, mut indent, mut cur, mut in_cmds): (bool, i64, i64, i64) = (false, -1, -1, -1);
    for l in lines(&first_existing(
        root,
        &["Taskfile.yml", "Taskfile.yaml", "taskfile.yml", "taskfile.yaml", "Taskfile.dist.yml", "Taskfile.dist.yaml"],
    )) {
        let t = l.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let ind = indent_of(&l) as i64;
        if ind == 0 {
            inside = t == "tasks:";
            cur = -1;
            continue;
        }
        if !inside {
            continue;
        }
        if indent < 0 {
            indent = ind;
        }
        if ind == indent {
            let Some((key, _)) = t.split_once(':') else { continue };
            let name = trim_quotes(key);
            let mut c = task(&format!("task {name}"), "", &name, &[]);
            if name == "default" {
                c.forms.push("=task".into());
            }
            out.push(c);
            cur = out.len() as i64 - 1;
        } else if cur >= 0 && ind > indent {
            let ci = cur as usize;
            if let Some(d) = t.strip_prefix("desc:") {
                if out[ci].desc.is_empty() {
                    out[ci].desc = trim_quotes(d.trim());
                }
            }
            if t.replace(' ', "") == "internal:true" {
                internal.insert(ci);
            }
            if let Some(c) = t.strip_prefix("cmd:") {
                bodies.insert(ci, trim_quotes(c.trim()));
            } else if t == "cmds:" {
                in_cmds = cur;
            } else if let Some(c) = t.strip_prefix("- ") {
                if in_cmds == cur && bodies.get(&ci).is_none_or(|b| b.is_empty()) && !c.contains(": ") {
                    bodies.insert(ci, trim_quotes(c.trim()));
                }
            }
        }
    }
    out.into_iter()
        .enumerate()
        .filter(|(i, _)| !internal.contains(i))
        .map(|(i, c)| c.running(bodies.get(&i).map_or("", |s| s.as_str())))
        .collect()
}

fn mise_tasks(root: &str) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut cur: i64 = -1;
    let mut hidden: HashSet<String> = HashSet::new();
    let mut in_table = false;
    let add = |out: &mut Vec<Candidate>, name: &str, desc: &str| -> i64 {
        let r = format!("mise r {name}");
        out.push(task(&format!("mise run {name}"), desc, name, &[&r]));
        out.len() as i64 - 1
    };
    let mut all = Vec::new();
    for p in [
        join(&[root, "mise.toml"]),
        join(&[root, ".mise.toml"]),
        join(&[root, "mise.local.toml"]),
        join(&[root, ".config", "mise.toml"]),
        join(&[root, ".config", "mise", "config.toml"]),
        join(&[root, "mise", "config.toml"]),
        join(&[root, ".mise", "config.toml"]),
    ] {
        all.extend(lines(&p));
    }
    for l in all {
        let t = l.trim();
        if t.starts_with('[') {
            in_table = t == "[tasks]";
            cur = -1;
            if let Some(name) = crate::pat::mise_section(t) {
                cur = add(&mut out, name, "");
            }
            continue;
        }
        if in_table {
            if let Some((name, m2)) = crate::pat::mise_key(t) {
                let mut desc = trim_quotes(m2);
                if let Some(d) = crate::pat::mise_desc(m2) {
                    desc = d.to_string();
                } else if m2.starts_with('{') {
                    desc = String::new();
                }
                cur = add(&mut out, name, &desc);
            }
        } else if cur >= 0 {
            let ci = cur as usize;
            if let Some(d) = crate::pat::mise_desc(t) {
                out[ci].desc = d.to_string();
            } else if let Some(r) = crate::pat::mise_run(t) {
                let c = std::mem::take(&mut out[ci]);
                out[ci] = c.running(r);
            }
            if t.replace(' ', "") == "hide=true" {
                hidden.insert(out[ci].cmd.clone());
            }
        }
    }
    out.into_iter().filter(|c| !hidden.contains(&c.cmd)).collect()
}

fn deno_tasks(root: &str) -> Vec<Candidate> {
    let path = first_existing(root, &["deno.json", "deno.jsonc"]);
    let clean: Vec<String> = lines(&path).into_iter().filter(|l| !l.trim().starts_with("//")).collect();
    let Some(doc) = json::parse(&crate::gofs::bytes(&clean.join("\n"))) else { return vec![] };
    let Some(m) = json::members(Some(&doc)) else { return vec![] };
    let mut out = Vec::new();
    for (key, val) in json::ordered(json::raw(m, "tasks")) {
        let desc = match json::as_string(Some(val)) {
            Some(s) => s,
            None => {
                let (mut command, mut description) = (String::new(), String::new());
                if let J::Obj(om) = val {
                    command = json::string_field_lenient(om, "command");
                    description = json::string_field_lenient(om, "description");
                }
                if description.is_empty() {
                    command
                } else {
                    description
                }
            }
        };
        out.push(task(&format!("deno task {key}"), "", key, &[]).running(&desc));
    }
    out
}

fn composer_tasks(root: &str) -> Vec<Candidate> {
    let Some(data) = read_file(&join(&[root, "composer.json"])) else { return vec![] };
    let Some(doc) = json::parse(&data) else { return vec![] };
    let Some(m) = json::members(Some(&doc)) else { return vec![] };
    let (require, scripts) = (json::raw(m, "require"), json::raw(m, "scripts"));
    let mut descs: HashMap<String, String> = HashMap::new();
    for (k, v) in m {
        if !k.eq_ignore_ascii_case("scripts-descriptions") {
            continue;
        }
        match v {
            J::Null => descs.clear(), // null makes a Go map nil
            J::Obj(dm) => {
                for (dk, dv) in dm {
                    match json::as_string(Some(dv)) {
                        Some(s) => {
                            descs.insert(dk.clone(), s);
                        }
                        None => return vec![],
                    }
                }
            }
            _ => return vec![],
        }
    }
    let mut out = Vec::new();
    if !json::ordered(require).is_empty() {
        let vendor = join(&[root, "vendor"]);
        let lock = join(&[root, "composer.lock"]);
        if !exists(&vendor) {
            out.push(setup("composer install"));
        } else if stale_since(&lock, &vendor) {
            let mut s = setup("composer install");
            s.desc = "composer.lock changed since the last install".into();
            out.push(s);
        }
    }
    for (key, val) in json::ordered(scripts) {
        if key.starts_with("pre-") || key.starts_with("post-") {
            continue;
        }
        let mut desc = descs.get(key).cloned().unwrap_or_default();
        if desc.is_empty() {
            desc = match json::as_string(Some(val)) {
                Some(s) => s,
                None => json::as_strings(Some(val)).map(|v| v.join(" && ")).unwrap_or_default(),
            };
        }
        let (a, b) = (format!("composer {key}"), format!("composer run-script {key}"));
        out.push(task(&format!("composer run {key}"), &desc, key, &[&a, &b]));
    }
    out
}

fn cargo_tasks(root: &str) -> Vec<Candidate> {
    let manifest = join(&[root, "Cargo.toml"]);
    if !exists(&manifest) {
        return vec![];
    }
    let mut out = Vec::new();
    let mut in_alias = false;
    for l in lines(&first_existing(&join(&[root, ".cargo"]), &["config.toml", "config"])) {
        let t = l.trim();
        if t.starts_with('[') {
            in_alias = t == "[alias]";
            continue;
        }
        if let (true, Some((k, v))) = (in_alias, t.split_once('=')) {
            let name = trim_quotes(k.trim());
            let desc = v.trim().trim_matches(|c| "\"'[]".contains(c));
            out.push(task(&format!("cargo {name}"), desc, &format!("alias:{name}"), &[]));
        }
    }
    if exists(&join(&[root, "src", "main.rs"]))
        || exists(&join(&[root, "src", "bin"]))
        || contains(&read_file(&manifest), b"[[bin]]")
    {
        out.push(toolchain("cargo run", "run", &[]));
    }
    out.push(task("cargo test", "", "test", &[]));
    out.push(toolchain("cargo build", "build", &["cargo install"]));
    out
}

fn go_tasks(root: &str) -> Vec<Candidate> {
    if !exists(&join(&[root, "go.mod"])) {
        return vec![];
    }
    let mut out = Vec::new();
    if contains(&read_file(&join(&[root, "main.go"])), b"package main") {
        out.push(toolchain("go run .", "run", &[]));
    }
    note(&join(&[root, "cmd"]));
    for m in glob(&join(&[root, "cmd", "*", "main.go"])).iter().take(5) {
        let name = base(&crate::gofs::dir(m));
        out.push(task(&format!("go run ./cmd/{name}"), "", &name, &[]));
    }
    out.push(task("go test ./...", "", "test", &[]));
    out.push(toolchain("go build ./...", "build", &["go install"]));
    out
}

fn toolchain(cmd: &str, name: &str, also: &[&str]) -> Candidate {
    let mut c = task(cmd, "", name, &[]);
    let words = fields(cmd);
    c.verbs = vec![format!("{} {}", words[0], words[1])];
    c.verbs.extend(also.iter().map(|s| s.to_string()));
    c.forms.extend(c.verbs.clone());
    c
}

fn python_tasks(root: &str) -> Vec<Candidate> {
    let pyproject = read_file(&join(&[root, "pyproject.toml"]));
    let uses_pytest = contains(&pyproject, b"pytest")
        || !first_existing(root, &["pytest.ini", "conftest.py"]).is_empty()
        || contains(&read_file(&join(&[root, "tox.ini"])), b"[pytest]");
    let mut out = Vec::new();
    if pyproject.is_some() && exists(&join(&[root, "uv.lock"])) {
        let (lock, venv) = (join(&[root, "uv.lock"]), join(&[root, ".venv"]));
        if !exists(&venv) {
            out.push(setup("uv sync"));
        } else if stale_since(&lock, &venv) {
            let mut s = setup("uv sync");
            s.desc = "uv.lock changed since the last sync".into();
            out.push(s);
        }
        if uses_pytest {
            out.push(task("uv run pytest", "", "test", &["pytest"]));
        }
    } else if pyproject.is_some() && exists(&join(&[root, "poetry.lock"])) {
        out.push(task("poetry install", "", "setup", &[]));
        if uses_pytest {
            out.push(task("poetry run pytest", "", "test", &["pytest"]));
        }
    } else if uses_pytest {
        out.push(task("pytest", "", "test", &["python -m pytest"]));
    }
    out
}

fn jvm_tasks(root: &str) -> Vec<Candidate> {
    if exists(&join(&[root, "gradlew"])) {
        return vec![
            task("./gradlew build", "", "build", &["gradlew build", "gradle build"]),
            task("./gradlew test", "", "test", &["gradlew test", "gradle test"]),
        ];
    }
    if !first_existing(root, &["build.gradle", "build.gradle.kts"]).is_empty() {
        return vec![task("gradle build", "", "build", &[]), task("gradle test", "", "test", &[])];
    }
    if exists(&join(&[root, "pom.xml"])) {
        let mvn = if exists(&join(&[root, "mvnw"])) { "./mvnw" } else { "mvn" };
        return vec![
            task(
                &format!("{mvn} package"),
                "",
                "build",
                &["mvn package", "mvnw package", "mvn clean package", "mvn install", "mvn clean install"],
            ),
            task(&format!("{mvn} test"), "", "test", &["mvn test", "mvnw test"]),
        ];
    }
    vec![]
}

fn mix_tasks(root: &str) -> Vec<Candidate> {
    let mixfile = read_file(&join(&[root, "mix.exs"]));
    if mixfile.is_none() {
        return vec![];
    }
    let mut out = Vec::new();
    if !exists(&join(&[root, "deps"])) {
        out.push(setup("mix deps.get"));
    }
    if contains(&mixfile, b":phoenix") {
        out.push(task("mix phx.server", "", "dev", &["iex -S mix phx.server"]));
    }
    out.push(task("mix test", "", "test", &[]));
    out.push(task("mix compile", "", "build", &[]));
    out
}

fn conventional_scripts(root: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    for s in [
        "bin/dev",
        "bin/setup",
        "script/bootstrap",
        "script/setup",
        "script/server",
        "script/test",
        "script/update",
        "script/console",
    ] {
        if exists(&join(&[root, s])) {
            out.push(task(s, "", &base(s), &[]));
        }
    }
    out
}

fn compose_tasks(root: &str) -> Vec<Candidate> {
    let file = first_existing(root, &["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"]);
    if file.is_empty() {
        return vec![];
    }
    let mut services: Vec<String> = Vec::new();
    let mut inside = false;
    for l in lines(&file) {
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if !l.starts_with(' ') {
            inside = l.starts_with("services:");
        } else if let Some((name, rest)) = l.strip_prefix("  ").unwrap_or(&l).split_once(':') {
            if inside && rest.is_empty() && !name.is_empty() && !name.starts_with(' ') && !name.starts_with('#') {
                services.push(name.to_string());
            }
        }
    }
    if services.len() > 5 {
        services.truncate(5);
        services.push("…".into());
    }
    vec![task("docker compose up", &services.join(", "), "compose", &["docker-compose up"])]
}
