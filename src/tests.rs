use crate::cache::{build, discovery_path, hints_path, unchanged, CACHE_FORMAT, HINTS_FORMAT};
use crate::gist::{gist, tool_stage};
use crate::history::{history_path, read_tail, HISTORY_MAX, HISTORY_TAIL};
use crate::suggest::{find_root, Suggestion, STAGE_RISKY, STAGE_VERIFY};
use crate::write_suggestions;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime};

const VARS: [&str; 3] = ["HOME", "PATH", "XDG_STATE_HOME"];

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn original_env() -> &'static Vec<(&'static str, Option<OsString>)> {
    static ORIG: OnceLock<Vec<(&'static str, Option<OsString>)>> = OnceLock::new();
    ORIG.get_or_init(|| VARS.iter().map(|&k| (k, std::env::var_os(k))).collect())
}

fn restore_env() {
    for (k, v) in original_env() {
        match v {
            Some(v) => std::env::set_var(k, v),
            None => std::env::remove_var(k),
        }
    }
}

struct Env {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for Env {
    fn drop(&mut self) {
        restore_env();
    }
}

fn env() -> Env {
    let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    original_env();
    restore_env();
    Env { _lock: lock }
}

struct TempDir(String);

impl TempDir {
    fn new() -> TempDir {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
        let p = std::env::temp_dir().join(format!("usual-test-{}-{n}-{nanos}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        TempDir(p.to_str().unwrap().to_string())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn join(a: &str, b: &str) -> String {
    Path::new(a).join(b).to_str().unwrap().to_string()
}

fn parent(p: &str) -> String {
    Path::new(p).parent().unwrap().to_str().unwrap().to_string()
}

fn chmod(p: &str, mode: u32) {
    fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
}

fn chtimes(p: &str, t: SystemTime) {
    let f = fs::File::open(p).unwrap();
    f.set_times(fs::FileTimes::new().set_accessed(t).set_modified(t)).unwrap();
}

fn hour_ago() -> SystemTime {
    SystemTime::now() - Duration::from_secs(3600)
}

fn project(tools: &[&str], files: &[(&str, &str)]) -> (TempDir, String) {
    let home = TempDir::new();
    std::env::set_var("HOME", &home.0);
    std::env::set_var("XDG_STATE_HOME", join(&home.0, ".state"));
    let bin = join(&home.0, "bin");
    fs::create_dir_all(&bin).unwrap();
    for tool in tools {
        let p = join(&bin, tool);
        fs::write(&p, "#!/bin/sh\n").unwrap();
        chmod(&p, 0o755);
    }
    std::env::set_var("PATH", &bin);
    let root = join(&home.0, "code/app");
    for (name, body) in files {
        let p = join(&root, name);
        fs::create_dir_all(parent(&p)).unwrap();
        if name.ends_with('/') {
            fs::create_dir_all(&p).unwrap();
            continue;
        }
        fs::write(&p, body).unwrap();
    }
    (home, root)
}

fn suggest_now(dir: &str) -> (String, Vec<Suggestion>) {
    let (root, list, _) = crate::suggest::suggest(dir, crate::history::now());
    (root, list)
}

fn cmds(dir: &str) -> Vec<String> {
    suggest_now(dir).1.into_iter().map(|s| s.cmd).collect()
}

#[track_caller]
fn expect(got: Vec<String>, want: &[&str]) {
    assert_eq!(got, want, "\n got {got:?}\nwant {want:?}");
}

fn first(got: &[String]) -> &str {
    got.first().map(String::as_str).unwrap_or("")
}

fn backdate(dir: &str) {
    fn walk(p: &Path, t: SystemTime) {
        if let Ok(md) = fs::symlink_metadata(p) {
            if md.is_dir() {
                if let Ok(rd) = fs::read_dir(p) {
                    for e in rd.flatten() {
                        walk(&e.path(), t);
                    }
                }
            }
            if !md.file_type().is_symlink() {
                if let Ok(f) = fs::File::open(p) {
                    let _ = f.set_times(fs::FileTimes::new().set_accessed(t).set_modified(t));
                }
            }
        }
    }
    walk(Path::new(dir), hour_ago());
}

#[test]
fn test_node() {
    let _env = env();
    let (_home, root) = project(
        &["pnpm"],
        &[
            (
                "package.json",
                r#"{"scripts": {
			"postinstall": "husky", "prebuild": "tsc", "build": "vite build", "dev": "vite",
			"lint": "eslint .", "deploy": "fly deploy", "test": "vitest", "weird name": "x",
			"add": "x", "_internal": "x"},
			"devDependencies": {"vite": "5"}}"#,
            ),
            ("pnpm-lock.yaml", ""),
            ("README.md", "```sh\n$ pnpm install\n$ pnpm build && pnpm lint\n```\n"),
        ],
    );
    expect(
        cmds(&root),
        &["pnpm install", "pnpm dev", "pnpm build", "pnpm lint", "pnpm test", "pnpm run add", "pnpm deploy"],
    );

    fs::create_dir_all(join(&root, "node_modules")).unwrap();
    let modules = join(&root, "node_modules/.modules.yaml");
    fs::write(&modules, "").unwrap();
    let old = hour_ago();
    chtimes(&modules, old);
    let got = cmds(&root);
    assert_eq!(first(&got), "pnpm install", "lockfile newer than the install: want pnpm install first, got {got:?}");
    chtimes(&join(&root, "pnpm-lock.yaml"), old - Duration::from_secs(3600));
    let got = cmds(&root);
    assert_eq!(first(&got), "pnpm dev", "installed and up to date: want no install, got {got:?}");
}

#[test]
fn test_npm_forms() {
    let _env = env();
    let (_home, root) = project(
        &["npm"],
        &[
            ("package.json", r#"{"scripts": {"start": "node .", "test": "jest", "storybook": "sb"}}"#),
            ("README.md", "Run `yarn storybook` to see components."),
        ],
    );
    expect(cmds(&root), &["npm start", "npm test", "npm run storybook"]);
}

#[test]
fn test_missing_tools_and_bad_files() {
    let _env = env();
    let (_home, root) = project(
        &["make"],
        &[
            ("package.json", r#"{"scripts": {"dev": "vite""#),
            ("justfile", "build:\n    echo\n"),
            ("Makefile", "test:\n\tgo test\n"),
        ],
    );
    expect(cmds(&root), &["make test"]);
}

#[test]
fn test_just() {
    let _env = env();
    let (_home, root) = project(
        &["just"],
        &[(
            "justfile",
            r#"set shell := ["bash", "-c"]
version := "1"
alias b := build

# Build it
build target="all":
    echo

[private]
helper:
    echo

[doc('Ship it')]
release: build
    echo

_hidden:
    echo

# Start the device on a page, e.g. a test app. arch is arm or x64,
# and memory is its size.
up:
    echo

# ---------- Tools ----------
# Format the code.
fmt:
    echo
"#,
        )],
    );
    let (_, list) = suggest_now(&root);
    let desc = |c: &str| list.iter().find(|s| s.cmd == c).map(|s| s.desc.clone()).unwrap_or_default();
    let mut got = cmds(&root);
    got.sort();
    expect(got, &["just build", "just fmt", "just release", "just up"]);
    assert!(desc("just build") == "Build it" && desc("just release") == "Ship it", "docs: {list:?}");
    assert_eq!(desc("just up"), "Start the device on a page, e.g. a test app");
    assert_eq!(desc("just fmt"), "Format the code");
}

#[test]
fn test_first_sentence() {
    use crate::first_sentence as f;
    assert_eq!(f("Build it."), "Build it");
    assert_eq!(f("Build it. Then ship it."), "Build it");
    assert_eq!(f("Use the  x64 image, i.e. faster. Slow on arm."), "Use the x64 image, i.e. faster");
    assert_eq!(f("Wait for it..."), "Wait for it...");
    assert_eq!(f("Serve on http://localhost:8080/a.html."), "Serve on http://localhost:8080/a.html");
    assert_eq!(f("Step 1. Build"), "Step 1. Build");
}

#[test]
fn test_make() {
    let _env = env();
    let (_home, root) = project(
        &["make"],
        &[(
            "Makefile",
            "CC := gcc
X = a:b
.PHONY: test lint
## Run the tests. Needs Go,
## and a network for the first run.
test:
\tgo test
lint: ## Lint the code
\tgolangci-lint run
build/out.o: x.c
\tcc
%.o: %.c
\tcc
helper:
\ttrue
",
        )],
    );
    let (_, list) = suggest_now(&root);
    expect(cmds(&root), &["make test", "make lint"]);
    assert!(list[0].desc == "Run the tests" && list[1].desc == "Lint the code", "docs: {list:?}");
}

#[test]
fn test_taskfile_and_mise() {
    let _env = env();
    let (_home, root) = project(
        &["task", "mise"],
        &[
            (
                "Taskfile.yml",
                r#"version: '3'
tasks:
  build:
    desc: Build it
    cmds: [go build]
  gen:
    internal: true
  "serve":
    cmds: [x]
"#,
            ),
            (
                "mise.toml",
                r#"[tools]
node = "22"
[tasks]
fmt = "prettier -w ."
check = { run = "tsc", description = "Type check" }
[tasks.dev]
description = "Dev server"
run = "vite"
[tasks.secret]
hide = true
"#,
            ),
        ],
    );
    expect(cmds(&root), &["mise run dev", "task serve", "task build", "mise run check", "mise run fmt"]);
}

#[test]
fn test_toolchains() {
    let _env = env();
    let (_home, root) = project(
        &["go", "docker", "git", "cp"],
        &[
            ("go.mod", "module x"),
            ("main.go", "package main"),
            ("cmd/tool/main.go", "package main"),
            ("compose.yaml", ""),
            (".env.example", ""),
            (".gitmodules", "[submodule \"lib\"]\n\tpath = vendor/lib\n"),
            ("README.md", "Run `git submodule update --init` first."),
            ("vendor/lib/", ""),
        ],
    );
    expect(
        cmds(&root),
        &[
            "git submodule update --init --recursive",
            "cp .env.example .env",
            "go run .",
            "go build ./...",
            "go test ./...",
            "go run ./cmd/tool",
            "docker compose up",
        ],
    );

    let sub = join(&root, "internal");
    fs::create_dir_all(&sub).unwrap();
    expect(cmds(&sub), &["docker compose up"]);
}

#[test]
fn test_families_and_noise() {
    let _env = env();
    let (_home, root) = project(
        &["npm"],
        &[(
            "package.json",
            r#"{"scripts": {
			"test": "echo \"Error: no test specified\" && exit 1",
			"dev": "x", "dev-esm": "x", "dev:sfc": "x", "test:unit": "x", "test-ci": "x",
			"build": "x", "build:prod": "x", "help": "x", "lint": "x"}}"#,
        )],
    );
    expect(
        cmds(&root),
        &[
            "npm run dev",
            "npm run build",
            "npm run test:unit",
            "npm run lint",
            "npm run help",
            "npm run dev-esm",
            "npm run dev:sfc",
            "npm run test-ci",
            "npm run build:prod",
        ],
    );
}

#[test]
fn test_readme_sections() {
    let _env = env();
    let (_home, root) = project(
        &["just"],
        &[
            ("justfile", "lint:\n    x\ntypecheck:\n    x\n"),
            ("README.md", "# just\n## Usage\nRun `just lint` to check.\n## Development\n```\njust typecheck\n```\n"),
        ],
    );
    expect(cmds(&root), &["just typecheck", "just lint"]);
}

#[test]
fn test_scripts_and_python() {
    let _env = env();
    let (_home, root) = project(
        &["pytest"],
        &[
            ("Gemfile", ""),
            ("conftest.py", ""),
            ("pyproject.toml", "[project]"),
            ("bin/dev", "#!/bin/sh"),
            ("bin/setup", "#!/bin/sh"),
            ("script/test", "#!/bin/sh"),
        ],
    );
    chmod(&join(&root, "bin/dev"), 0o755);
    chmod(&join(&root, "bin/setup"), 0o755);
    expect(cmds(&root), &["bin/dev", "bin/setup", "pytest"]);
}

#[test]
fn test_history() {
    let _env = env();
    let (_home, root) = project(
        &["pnpm"],
        &[
            ("package.json", r#"{"scripts": {"dev": "vite", "test": "vitest", "build": "vite build", "e2e": "pw"}}"#),
            ("pnpm-lock.yaml", ""),
        ],
    );
    let now = crate::history::unix(crate::history::now());
    let mut h = String::new();
    let at = |h: &mut String, ago: i64, cmd: &str| {
        h.push_str(&format!("{}\t{root}\t{cmd}\n", now - ago));
    };
    const HOUR: i64 = 3600;
    at(&mut h, 72 * HOUR, "pnpm e2e");
    at(&mut h, 71 * HOUR, "pnpm build");
    at(&mut h, 71 * HOUR - 60, "pnpm test");
    at(&mut h, 48 * HOUR, "pnpm e2e");
    at(&mut h, 47 * HOUR, "pnpm build");
    at(&mut h, 47 * HOUR - 60, "pnpm test");
    h.push_str(&format!("garbage line\n{now}\t/elsewhere\tpnpm dev\n"));
    fs::create_dir_all(parent(&history_path())).unwrap();
    fs::write(history_path(), &h).unwrap();

    let got = cmds(&root);
    assert_eq!(first(&got), "pnpm e2e", "new session: want pnpm e2e first, got {got:?}");
    at(&mut h, 60, "pnpm build");
    fs::write(history_path(), &h).unwrap();
    let got = cmds(&root);
    assert_eq!(first(&got), "pnpm test", "after build: want pnpm test first, got {got:?}");
}

#[test]
fn test_history_trim() {
    let _env = env();
    let _home = project(&[], &[]);
    fs::create_dir_all(parent(&history_path())).unwrap();
    let line = "x".repeat(99) + "\n";
    fs::write(history_path(), line.repeat((HISTORY_MAX / 100 + 100) as usize)).unwrap();
    read_tail(&history_path());
    let size = fs::metadata(history_path()).unwrap().len();
    assert!(size <= HISTORY_TAIL, "history not trimmed: {size} bytes");
}

#[test]
fn test_root() {
    let _env = env();
    let (_home, root) = project(&[], &[(".git/HEAD", ""), ("src/deep/x.txt", "")]);
    let r = find_root(&join(&root, "src/deep"));
    assert_eq!(r, root, "subfolder: got {r:?}, want {root:?}");
    let home = std::env::var("HOME").unwrap();
    fs::write(join(&home, "Makefile"), "x:\n").unwrap();
    let r = find_root(&home);
    assert_eq!(r, "", "$HOME is never a project, got {r:?}");
    let r = find_root(&join(&home, "code"));
    assert_eq!(r, "", "folder under $HOME with no project: got {r:?}");
}

#[test]
fn test_stages() {
    let _env = env();
    let (_home, root) = project(
        &["yarn"],
        &[
            (
                "package.json",
                r#"{"scripts": {"test": "jest", "lint": "eslint", "dx": "turbo dx",
			"build": "tsc", "dev": "next", "db:reset": "x", "storybook": "sb"}}"#,
            ),
            ("yarn.lock", ""),
            ("README.md", "## Getting started\n```\nyarn test\nyarn lint\nyarn dx\n```\n"),
        ],
    );
    expect(
        cmds(&root),
        &["yarn dx", "yarn dev", "yarn build", "yarn test", "yarn lint", "yarn storybook", "yarn db:reset"],
    );
}

#[test]
fn test_cache() {
    let _env = env();
    let (_home, root) = project(&["npm", "just"], &[("package.json", r#"{"scripts": {"dev": "vite"}}"#)]);
    backdate(&parent(&parent(&root)));
    expect(cmds(&root), &["npm run dev"]);

    let path = discovery_path(&root);
    let data = fs::read_to_string(&path).unwrap_or_else(|e| panic!("no cache written: {e}"));
    fs::write(&path, data.replace("npm run dev", "npm run cached")).unwrap();
    expect(cmds(&root), &["npm run cached"]);

    fs::write(join(&root, "package.json"), r#"{"scripts": {"dev": "vite", "test": "vitest"}}"#).unwrap();
    expect(cmds(&root), &["npm run dev", "npm test"]);
    backdate(&root);
    cmds(&root);
    fs::write(join(&root, "justfile"), "build:\n    x\n").unwrap();
    expect(cmds(&root), &["npm run dev", "just build", "npm test"]);

    let three = format!("{CACHE_FORMAT} {}\n{root}\nC\tonly\tthree\n", build());
    for junk in ["", "garbage", three.as_str()] {
        fs::write(&path, junk).unwrap();
        expect(cmds(&root), &["npm run dev", "just build", "npm test"]);
    }
}

#[test]
fn test_hints() {
    let _env = env();
    let (_home, root) = project(&["npm"], &[("package.json", r#"{"scripts": {"dev": "vite"}}"#)]);
    backdate(&parent(&parent(&root)));
    write_suggestions(&root);
    // write_suggestions silences panic messages for the rest of the process; put them back.
    let _ = std::panic::take_hook();
    let data = fs::read_to_string(hints_path(&root)).unwrap_or_else(|e| panic!("no hints written: {e}"));
    let lines: Vec<&str> = data.split('\n').collect();
    let n: usize = lines[3].parse().unwrap_or(0);
    assert!(
        lines[0] == HINTS_FORMAT && lines[2] == std::env::var("PATH").unwrap() && n != 0,
        "bad header: {:?}",
        &lines[..4]
    );
    for i in (4..4 + 2 * n).step_by(2) {
        let (p, stamp) = (lines[i], lines[i + 1]);
        assert!(stamp == "?" || unchanged(p, stamp), "dependency {p} stamped {stamp} doesn't match");
    }
    let got = lines[4 + 2 * n..].join("\n");
    assert_eq!(got, format!("{root}\n1\nnpm run dev\nvite\n"), "output: {got:?}");
}

#[test]
fn test_machine_docs() {
    let _env = env();
    let (_home, root) = project(
        &["pnpm", "cargo"],
        &[
            (
                "package.json",
                r#"{"scripts": {"dev": "vite", "playground": "sb", "lint": "eslint", "typecheck": "tsc",
			"build": "just build", "e2e": "pw"}}"#,
            ),
            ("pnpm-lock.yaml", ""),
            (".gitpod.yml", "tasks:\n  - init: pnpm install\n    command: |\n      pnpm playground\n"),
            (".github/workflows/ci.yml", "jobs:\n  ci:\n    steps:\n      - run: pnpm typecheck --noEmit\n"),
            ("Cargo.toml", "[package]"),
            (".cargo/config.toml", "[alias]\nxtask = \"run --package xtask --\"\n"),
        ],
    );
    expect(
        cmds(&root),
        &[
            "pnpm playground",
            "pnpm dev",
            "pnpm build",
            "cargo test",
            "pnpm e2e",
            "pnpm typecheck",
            "pnpm lint",
            "cargo xtask",
            "cargo build",
        ],
    );
}

#[test]
fn test_passthrough_and_deno() {
    let _env = env();
    let (_home, root) = project(
        &["npm", "just", "deno"],
        &[
            ("package.json", r#"{"scripts": {"build": "just build", "start": "npm run dev", "dev": "vite"}}"#),
            ("justfile", "build:\n    x\n"),
        ],
    );
    expect(cmds(&root), &["npm run dev", "npm run build"]);

    let (_home2, root) = project(
        &["deno"],
        &[(
            "package.json",
            r#"{"packageManager": "deno@2.1.0", "scripts": {"dev": "vite"}, "dependencies": {"x": "1"}}"#,
        )],
    );
    expect(cmds(&root), &["deno task dev"]);
}

#[test]
fn test_untrusted_folders() {
    let _env = env();
    let (_home, root) = project(&["make"], &[("Makefile", "test:\n\tx\n"), ("sub/x", "")]);
    chmod(&root, 0o777);
    let r = find_root(&join(&root, "sub"));
    assert_ne!(r, root, "took a world-writable folder for the project");
}

#[test]
fn test_gist() {
    for (input, want) in [
        ("NODE_OPTIONS=--max-old-space-size=4096 cross-env FOO=1 next dev -p 3000", "next dev"),
        ("dotenv -e .env.local -- vite --port 3000", "vite"),
        ("vite", "vite"),
        ("rm -rf dist && tsc -b && vite build --mode lib", "rm, tsc, vite build"),
        ("cd apps/web && next dev -p 3000", "apps/web: next dev"),
        ("pnpm --filter @acme/web dev", "@acme/web: dev"),
        ("yarn workspace docs start", "docs: start"),
        ("npm run build:lib && npm run build:docs", "build:lib, build:docs"),
        ("npm run dev", "npm run dev"),
        ("echo building && npx tsup src/index.ts --dts", "tsup"),
        ("a && b && c && d && e && f", "a, b, +4"),
        ("node --expose-gc bench/bench.ts", "bench.ts"),
        ("npm run build && node packages/server/bin/teleport.js examples/hello-device", "build, teleport.js"),
        ("node scripts/gen.mjs && tsc && npm run build -w @a/agent && npm run build -w a", "gen.mjs, tsc, +2"),
        ("concurrently \"next dev --turbo\" \"tailwindcss --input in.css --watch\"", "next dev, tailwindcss"),
        ("python -m http.server 8000", "http.server"),
        ("go test ./... -race", "go test"),
        ("./hack/generate.sh", "generate.sh"),
        ("cd a && make && cd ../b && make", "2 dirs: make"),
        ("node --test \"test/*.test.ts\"", "node --test"),
    ] {
        let got = gist(input);
        assert_eq!(got, want, "gist({input:?}) = {got:?}, want {want:?}");
    }
}

#[test]
fn test_tool_stage() {
    for (input, want) in [
        ("playwright test --ui", STAGE_VERIFY),
        ("cross-env CI=1 vitest run", STAGE_VERIFY),
        ("pnpm build && vitest run", STAGE_VERIFY),
        ("turbo run lint --filter=web", STAGE_VERIFY),
        ("nx run-many -t test", STAGE_VERIFY),
        ("npm run build && npm publish", STAGE_RISKY),
        ("turbo run dev", 0),
        ("go run ./tools/gen", 0),
        ("node scripts/whatever.js", 0),
        ("rm -rf dist && tsc -b && vite build", 0),
    ] {
        let got = tool_stage(input).unwrap_or(0);
        assert_eq!(got, want, "tool_stage({input:?}) = {got}, want {want}");
    }
}

#[test]
fn test_project_clis() {
    let _env = env();
    let (_home, root) = project(
        &["acme", "yarn"],
        &[
            ("package.json", r#"{"name": "@co/acme", "bin": {"acme": "./cli.js"}, "scripts": {"lint": "eslint ."}}"#),
            ("yarn.lock", ""),
            ("bin/tool", "#!/bin/sh"),
            (
                "README.md",
                concat!(
                    "## Usage\n```\nacme generate thing\n```\n",
                    "## Development\n```\nacme run start\nacme test --watch\nacme <command>\n./bin/tool migrate\n```\n"
                ),
            ),
        ],
    );
    chmod(&join(&root, "bin/tool"), 0o755);
    expect(cmds(&root), &["acme run start", "bin/tool migrate", "acme test --watch", "yarn lint"]);
}

#[test]
fn test_descriptions() {
    let _env = env();
    let (_home, root) = project(
        &["make", "just", "task", "docker"],
        &[
            (
                "Makefile",
                concat!(
                    "GO ?= go\n##@ Development\n\n.PHONY: test lint docs gen\ntest:\n\t@echo testing\n\t$(GO) test ./... -race\n",
                    "lint: ## Lint the code\n\tgolangci-lint run\ndocs gen:\n\t./hack/generate.sh\n"
                ),
            ),
            ("justfile", "# Build it\nbuild:\n    cargo build\n\ne2e:\n    #!/usr/bin/env bash\n    @npx playwright test\n"),
            (
                "Taskfile.yml",
                "version: '3'\ntasks:\n  proto:\n    cmds:\n      - buf generate\n      - go mod tidy\n  site:\n    desc: Serve the docs\n    cmd: hugo serve\n",
            ),
            ("compose.yaml", "services:\n  db:\n    image: postgres\n  web:\n    build: .\nvolumes:\n  data:\n"),
        ],
    );
    let (_, list) = suggest_now(&root);
    let got: std::collections::HashMap<String, String> = list.into_iter().map(|s| (s.cmd, s.desc)).collect();
    for (cmd, want) in [
        ("make test", "go test"),
        ("make lint", "Lint the code"),
        ("make docs", "generate.sh"),
        ("make gen", "generate.sh"),
        ("just build", "Build it"),
        ("just e2e", "playwright test"),
        ("task proto", "buf generate"),
        ("task site", "Serve the docs"),
        ("docker compose up", "db, web"),
    ] {
        let have = got.get(cmd).map(String::as_str).unwrap_or("");
        assert_eq!(have, want, "{cmd}: described as {have:?}, want {want:?}");
    }
}

#[test]
fn test_spelled_out() {
    let _env = env();
    let home = TempDir::new();
    std::env::set_var("HOME", &home.0);
    let bin = join(&home.0, "bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(join(&bin, "go"), "#!/bin/sh\n").unwrap();
    chmod(&join(&bin, "go"), 0o755);
    std::env::set_var("PATH", &bin);
    let dir = TempDir::new();
    let root = dir.0.clone();
    fs::create_dir(join(&root, ".git")).unwrap();
    fs::write(join(&root, "go.mod"), "module x\n").unwrap();
    fs::write(join(&root, "main.go"), "package main\nfunc main() {}\n").unwrap();
    fs::write(
        join(&root, "README.md"),
        "## Install\n\n```\ngit clone https://example.com/x && cd x && go build -o ~/.local/bin/x .\n```\n",
    )
    .unwrap();
    let (_, got) = suggest_now(&root);
    assert!(!got.is_empty() && got[0].cmd == "go build -o ~/.local/bin/x .", "got {got:?}");
    fs::write(join(&root, "README.md"), "## Development\n\n```\ngo install ./cmd/x\n```\n").unwrap();
    let (_, got) = suggest_now(&root);
    assert!(!got.is_empty() && got[0].cmd == "go install ./cmd/x", "got {got:?}");
    fs::write(join(&root, "README.md"), "## Development\n\n```\ngo build $(pwd) | tee log\n```\n").unwrap();
    let (_, got) = suggest_now(&root);
    for s in &got {
        assert!(!s.cmd.contains('$'), "unsafe: {}", s.cmd);
    }
}

// USUAL_CORPUS=~/corpus cargo test --release corpus -- --nocapture

fn corpus() -> Option<Vec<String>> {
    let dir = std::env::var("USUAL_CORPUS").ok().filter(|d| !d.is_empty())?;
    let mut repos: Vec<String> = fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    repos.sort();
    Some(repos)
}

#[test]
fn test_corpus_report() {
    use crate::suggest::{discover_fresh, doc_commands, suggest};
    use std::time::Instant;
    let Some(repos) = corpus() else { return };
    let _env = env();
    let time_it = |f: &mut dyn FnMut()| {
        let start = Instant::now();
        for _ in 0..20 {
            f();
        }
        start.elapsed() / 20
    };
    let progs: Vec<String> =
        ["npm", "yarn", "pnpm", "bun", "just", "make", "cargo", "go", "docker"].iter().map(|s| s.to_string()).collect();
    let mut rows = Vec::new();
    for r in &repos {
        let mut parts: Vec<(&str, Duration)> = vec![
            ("docs", time_it(&mut || drop(doc_commands(r, &progs)))),
            ("history", time_it(&mut || drop(crate::history::project_usage(r, crate::history::now())))),
        ];
        for (name, read) in crate::sources::SOURCES {
            parts.push((name, time_it(&mut || drop(read(r)))));
        }
        let fresh = time_it(&mut || drop(discover_fresh(r)));
        suggest(r, crate::history::now());
        let total = time_it(&mut || drop(suggest(r, crate::history::now())));
        rows.push((r.rsplit('/').next().unwrap().to_string(), total, fresh, parts));
    }
    let stats = |mut v: Vec<Duration>| {
        v.sort();
        (v[v.len() / 2], v[v.len() * 9 / 10], v[v.len() - 1])
    };
    let (m, p, x) = stats(rows.iter().map(|r| r.1).collect());
    println!("{} projects, cached: median {m:?}, p90 {p:?}, max {x:?}", rows.len());
    let (m, p, x) = stats(rows.iter().map(|r| r.2).collect());
    println!("{} projects, fresh:  median {m:?}, p90 {p:?}, max {x:?}", rows.len());
    rows.sort_by_key(|r| std::cmp::Reverse(r.2));
    for (name, total, fresh, parts) in rows.iter().take(10) {
        let mut top: Vec<String> = parts
            .iter()
            .filter(|(_, d)| *d > Duration::from_micros(50))
            .map(|(n, d)| format!("{n} {}µs", d.as_micros()))
            .collect();
        top.sort();
        println!("{name:<32} {:>6}µs  fresh {}µs, {}", total.as_micros(), fresh.as_micros(), top.join(", "));
    }
}

#[test]
fn test_corpus_agreement() {
    use crate::history::ProjectUse;
    use crate::suggest::*;
    use std::collections::BTreeMap;
    let Some(repos) = corpus() else { return };
    let _env = env();
    let stage_name = |s: i64| match s {
        STAGE_RUN => "run",
        STAGE_SETUP => "set up",
        STAGE_BUILD => "build",
        STAGE_VERIFY => "verify",
        STAGE_OTHER => "other",
        STAGE_MINOR => "minor",
        STAGE_RISKY => "risky",
        _ => "?",
    };
    let mut leads: BTreeMap<&str, usize> = BTreeMap::new();
    let mut places: Vec<usize> = Vec::new();
    let (mut tasks, mut placed, mut described) = (0, 0, 0);
    for r in &repos {
        let cs = discover_fresh(r);
        for c in cs.iter().filter(|c| c.score < SETUP_SCORE) {
            tasks += 1;
            placed += c.known as usize;
            described += !c.desc.is_empty() as usize;
        }
        let target =
            (0..cs.len()).filter(|&i| cs[i].dev_pos > 0 && cs[i].score < SETUP_SCORE).min_by_key(|&i| cs[i].dev_pos);
        let Some(target) = target else { continue };
        let want = cs[target].cmd.clone();
        *leads.entry(stage_name(cs[target].stage)).or_default() += 1;
        let mut blind = cs.clone();
        for c in &mut blind {
            (c.dev_pos, c.other_pos, c.check_pos) = (0, 0, 0);
        }
        rank(&mut blind, &ProjectUse::default());
        let mut place = 0;
        for c in blind.iter().filter(|c| c.score < SETUP_SCORE) {
            place += 1;
            if c.cmd == want {
                break;
            }
        }
        places.push(place);
    }
    if places.is_empty() {
        return;
    }
    places.sort();
    let within = |n: usize| 100 * places.iter().filter(|&&p| p <= n).count() / places.len();
    println!("{} of {} projects have a README command to look for", places.len(), repos.len());
    println!(
        "ranked without the docs, it comes first in {}%, in the first four in {}%, in the first eight in {}% (median place {})",
        within(1),
        within(4),
        within(8),
        places[places.len() / 2]
    );
    println!("what READMEs lead with, by stage: {leads:?}");
    println!(
        "of {tasks} commands, the name or tool gives a stage for {}%, and {}% have a description",
        100 * placed / tasks.max(1),
        100 * described / tasks.max(1)
    );
}
