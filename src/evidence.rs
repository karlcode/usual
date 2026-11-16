use crate::gofs::{base, glob, join, note};
use crate::json::{self, J};
use crate::sources::{lines, read_file};

pub fn setup_commands(root: &str) -> Vec<String> {
    let mut out = yaml_values(&join(&[root, ".gitpod.yml"]), &["before", "init", "command"]);
    let mut paths = vec![join(&[root, ".devcontainer", "devcontainer.json"]), join(&[root, ".devcontainer.json"])];
    note(&join(&[root, ".devcontainer"]));
    paths.extend(glob(&join(&[root, ".devcontainer", "*", "devcontainer.json"])));
    for p in paths {
        let Some(data) = strip_comments(read_file(&p)) else { continue };
        let Some(doc) = json::parse(&data) else { continue };
        let Some(m) = json::members(Some(&doc)) else { continue };
        for k in
            ["onCreateCommand", "updateContentCommand", "postCreateCommand", "postStartCommand", "postAttachCommand"]
        {
            let v = m.iter().rev().find(|(mk, _)| mk == k).map(|(_, v)| v);
            out.extend(json_commands(v));
        }
        break;
    }
    out
}

pub fn check_commands(root: &str) -> Vec<String> {
    let mut out = Vec::new();
    note(&join(&[root, ".github", "workflows"]));
    let mut read = 0;
    for w in glob(&join(&[root, ".github", "workflows", "*.y*ml"])) {
        if read < 5 && crate::pat::ci_workflow(&base(&w)) {
            out.extend(yaml_values(&w, &["run"]));
            read += 1;
        }
    }
    if let Some(tasks) = vscode_tasks(root) {
        out.extend(tasks);
    }
    out
}

fn vscode_tasks(root: &str) -> Option<Vec<String>> {
    let data = strip_comments(read_file(&join(&[root, ".vscode", "tasks.json"])))?;
    let doc = json::parse(&data)?;
    let m = json::members(Some(&doc))?;
    let mut list: Vec<Option<&J>> = Vec::new();
    for (k, v) in m {
        if !json::fold_eq(k, "tasks") {
            continue;
        }
        match v {
            J::Null => list.clear(),
            J::Arr(a) => list = a.iter().map(Some).collect(),
            _ => return None,
        }
    }
    let mut out = Vec::new();
    for t in list {
        let tm = json::members(t)?;
        let ty = json::string_field(tm, "type").ok()?;
        let script = json::string_field(tm, "script").ok()?;
        let command = json::string_field(tm, "command").ok()?;
        let mut args: Vec<String> = Vec::new();
        for (k, v) in tm {
            if json::fold_eq(k, "args") {
                args = json::as_strings(Some(v))?;
            }
        }
        if ty == "npm" && !script.is_empty() {
            out.push(format!("npm run {script}"));
        } else if !command.is_empty() {
            let mut parts = vec![command];
            parts.extend(args);
            out.push(parts.join(" "));
        }
    }
    Some(out)
}

pub fn yaml_values(path: &str, keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut block: i64 = -1;
    let mut ls = lines(path);
    ls.truncate(1500);
    for l in ls {
        let t = l.trim();
        let indent = (l.len() - l.trim_start_matches(' ').len()) as i64;
        if block >= 0 {
            if t.is_empty() || indent > block {
                if !t.is_empty() {
                    out.push(t.to_string());
                }
                continue;
            }
            block = -1;
        }
        let item = t.strip_prefix("- ").unwrap_or(t);
        for k in keys {
            let Some(v) = item.strip_prefix(&format!("{k}:")) else { continue };
            match v.trim() {
                "|" | "|-" | ">" | ">-" | "|+" => block = indent,
                "" => {}
                v => out.push(v.trim_matches(|c| c == '"' || c == '\'').to_string()),
            }
        }
    }
    out
}

fn json_commands(raw: Option<&J>) -> Vec<String> {
    if let Some(s) = json::as_string(raw) {
        return vec![s];
    }
    if let Some(argv) = json::as_strings(raw) {
        return vec![argv.join(" ")];
    }
    json::ordered(raw).iter().flat_map(|(_, v)| json_commands(Some(v))).collect()
}

fn strip_comments(data: Option<Vec<u8>>) -> Option<Vec<u8>> {
    let data = data?;
    let s = crate::gofs::text(&data);
    let kept: Vec<&str> = s.split('\n').filter(|l| !l.trim().starts_with("//")).collect();
    Some(crate::gofs::bytes(&kept.join("\n")))
}
