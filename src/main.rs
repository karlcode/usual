mod cache;
mod evidence;
mod gist;
mod gofs;
mod history;
mod json;
mod pat;
mod sources;
mod suggest;
mod tabwriter;
#[cfg(test)]
mod tests;

use std::io::Write;
use std::os::unix::fs::FileTypeExt;
use std::time::Duration;

const ZSH_INIT: &str = include_str!("../usual.plugin.zsh");

pub const VERSION: &str = match option_env!("USUAL_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

const USAGE: &str = r#"usage:
  usual [dir]        this project's suggested commands, best first
                       (in a pipe: command<TAB>description lines; --json for JSON)
  usual init zsh     the zsh integration; add  eval "$(usual init zsh)"  to ~/.zshrc
  usual version
"#;

fn main() {
    let argv: Vec<String> = std::env::args_os().skip(1).map(|a| gofs::os_text(&a)).collect();
    let mut args = argv.clone();
    args.push(String::new());
    let mut out = std::io::stdout().lock();
    match args[0].as_str() {
        "init" => {
            if args[1] != "zsh" {
                eprint!("usual: only zsh is supported\n\n{USAGE}");
                std::process::exit(2);
            }
            let _ = out.write_all(ZSH_INIT.as_bytes());
        }
        "suggest" => write_suggestions(&args[1]),
        "version" | "--version" | "-v" => {
            let _ = writeln!(out, "{VERSION}");
        }
        "help" | "--help" | "-h" => {
            let _ = out.write_all(USAGE.as_bytes());
        }
        _ => {
            let (mut d, mut as_json) = (".".to_string(), false);
            for a in &argv {
                if a == "--json" {
                    as_json = true;
                } else {
                    d = a.clone();
                }
            }
            if !gofs::stat(&d).is_some_and(|m| m.is_dir()) {
                eprint!("{USAGE}");
                std::process::exit(2);
            }
            print_list(&d, as_json);
        }
    }
}

fn print_list(d: &str, as_json: bool) {
    let abs = gofs::abs(d).unwrap_or_default();
    let (root, list, _) = suggest::suggest(&abs, history::now());
    if root.is_empty() {
        eprintln!("usual: not in a project");
        std::process::exit(1);
    }
    let mut s = String::new();
    if as_json {
        s.push_str("{\n  \"root\": ");
        json::quote(&root, &mut s);
        if list.is_empty() {
            s.push_str(",\n  \"commands\": []\n}\n");
        } else {
            s.push_str(",\n  \"commands\": [\n");
            for (i, c) in list.iter().enumerate() {
                s.push_str("    {\n      \"command\": ");
                json::quote(&c.cmd, &mut s);
                s.push_str(",\n      \"description\": ");
                json::quote(&one_line(&c.desc), &mut s);
                s.push_str(if i + 1 < list.len() { "\n    },\n" } else { "\n    }\n" });
            }
            s.push_str("  ]\n}\n");
        }
    } else if stdout_is_char_device() {
        let mut table = String::new();
        for c in &list {
            table += &format!("  {}\t{}\n", c.cmd, clip(&c.desc, 100));
        }
        let mut out = gofs::bytes(&format!("{root}\n"));
        out.extend(tabwriter::format(&gofs::bytes(&table)));
        let _ = std::io::stdout().lock().write_all(&out);
        return;
    } else {
        for c in &list {
            s.push_str(&format!("{}\t{}\n", c.cmd, one_line(&c.desc)));
        }
    }
    let _ = std::io::stdout().lock().write_all(&gofs::bytes(&s));
}

fn stdout_is_char_device() -> bool {
    use std::os::fd::FromRawFd;
    let f = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(1) });
    f.metadata().is_ok_and(|m| m.file_type().is_char_device())
}

/// Prints what usual.plugin.zsh reads: root, n, n commands, n descriptions.
fn write_suggestions(d: &str) {
    if d.is_empty() {
        return;
    }
    let Some(abs) = gofs::abs(d) else { return };
    std::panic::set_hook(Box::new(|_| {}));
    let (tx, rx) = std::sync::mpsc::channel();
    let a = abs.clone();
    let spawned = std::thread::Builder::new().stack_size(64 << 20).spawn(move || {
        gofs::swap_looked(Some(Default::default()));
        let h = history::history_path();
        if !h.is_empty() {
            cache::mkdir_all(&gofs::dir(&h));
        }
        let (root, list, expires) = suggest::suggest(&a, history::now());
        let mut out = String::new();
        if !root.is_empty() {
            out += &format!("{root}\n{}\n", list.len());
            for c in &list {
                out += &one_line(&c.cmd);
                out.push('\n');
            }
            for c in &list {
                out += &clip(&c.desc, 60);
                out.push('\n');
            }
        }
        for p in gofs::split_list(&gofs::getenv("PATH")) {
            gofs::note(p);
        }
        if let Ok(exe) = std::env::current_exe() {
            gofs::note(&gofs::os_text(exe.as_os_str()));
        }
        let looked = gofs::swap_looked(None).unwrap_or_default();
        let _ = tx.send((out, cache::dependencies(&looked), expires));
    });
    if spawned.is_err() {
        return;
    }
    let (out, deps, expires) = match rx.recv_timeout(Duration::from_millis(150)) {
        Ok(r) => r,
        Err(_) => return, // too slow, or it failed
    };
    let out = gofs::bytes(&out);
    let _ = std::io::stdout().lock().write_all(&out);
    cache::save_hints(&abs, &out, &deps, expires);
}

pub fn fields(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

pub fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn base_name(s: &str) -> String {
    gofs::base(s)
}

fn clip(s: &str, n: usize) -> String {
    let s = one_line(s);
    let r: Vec<char> = s.chars().collect();
    if r.len() <= n {
        return s;
    }
    let mut cut: String = gofs::go_runes(&r[..n - 1].iter().collect::<String>());
    if let Some(i) = cut.rfind(' ') {
        if i > cut.len() / 2 {
            cut.truncate(i);
        }
    }
    cut.trim_end_matches(|c| " ,;&|".contains(c)).to_string() + "…"
}
