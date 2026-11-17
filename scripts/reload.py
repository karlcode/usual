#!/usr/bin/env python3
"""scripts/reload.py: in tmux, a shell open across an upgrade picks up the new plugin at its next prompt, once."""
import os, shutil, subprocess, tempfile, time

repo = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
work = tempfile.mkdtemp(prefix="usual-reload-")
bin_dir = os.path.join(work, "bin")
os.makedirs(bin_dir)
subprocess.run(["cargo", "build", "--quiet", "--release"], cwd=repo, check=True)
real = os.path.join(work, "usual-real")
shutil.copy(os.path.join(repo, "target", "release", "usual"), real)
open(os.path.join(bin_dir, "pnpm"), "w").write('#!/bin/sh\necho "ran: $*"\n')
os.chmod(os.path.join(bin_dir, "pnpm"), 0o755)
project = os.path.join(work, "code", "webapp")
os.makedirs(os.path.join(project, ".git"))
os.makedirs(os.path.join(project, "node_modules"))
open(os.path.join(project, "package.json"), "w").write(
    '{"scripts":{"dev":"vite","build":"vite build","test":"vitest","lint":"eslint ."}}')
open(os.path.join(project, "pnpm-lock.yaml"), "w").write("")
old = time.time() - 3600
for root, dirs, files in os.walk(os.path.join(work, "code")):
    for f in dirs + files:
        os.utime(os.path.join(root, f), (old, old))

source = open(os.path.join(repo, "usual.plugin.zsh")).read()
# Counts loads, and draws the rule as ╰─ before the upgrade and ╰═ after it.
COUNT = "typeset -gi _usual_loads; (( ++_usual_loads )); typeset -g _usual_how=$zsh_eval_context[-1]\n"
OLD, NEW = "╰─ ", "╰═ "
STATE = ("w=(${(z)$(zstyle -L zle-line-pre-redraw)}); print -r -- how=$_usual_how loads=$_usual_loads precmd=${#${(M)precmd_functions:#_usual_*}}"
         " preexec=${#${(M)preexec_functions:#_usual_*}}"
         " redraw=${#${(M)w:#*_usual_draw*}}"
         " up=${(M)${(v)_usual_orig}:#history-beginning*} tab=${${(z)$(bindkey '^I')}[2]}")


def tmux(*args):
    return subprocess.run(["tmux", "-L", "reload", *args], capture_output=True, text=True).stdout


def screen():
    return tmux("capture-pane", "-t", "t", "-p")


def keys(*k, wait=0.8):
    tmux("send-keys", "-t", "t", *k)
    time.sleep(wait)


def write(path, text, at, compiled=False):
    open(path, "w").write(text)
    os.utime(path, (at, at))
    if compiled:   # as install.sh and brew do; zsh loads the .zwc only if it's the newer
        subprocess.run(["zsh", "-fc", 'zcompile -- "$1"', "zsh", path], check=True)
        os.utime(path + ".zwc", (at + 1, at + 1))


def run(mode):
    home = os.path.join(work, "home-" + mode)
    os.makedirs(home)
    plugin = os.path.join(home, "usual.plugin.zsh")
    usual = os.path.join(bin_dir, "usual")
    # `usual init zsh` prints the plugin file, so an upgrade can be staged by rewriting it.
    write(usual, f'#!/bin/sh\n[ "$1" = init ] && exec cat {plugin}\nexec {real} "$@"\n', old)
    os.chmod(usual, 0o755)
    write(plugin, COUNT + source, old, mode == "compiled")
    load = f"source {plugin}" if mode != "eval" else 'eval "$(usual init zsh)"'
    # A binding of the user's own under ↑, which usual must keep across the reload.
    open(os.path.join(home, ".zshrc"), "w").write(
        f"PS1='%1~ ❯ '\nbindkey '^[[A' history-beginning-search-backward\n{load}\n")
    env = (f"env -i HOME={home} ZDOTDIR={home} PATH={bin_dir}:/usr/local/bin:/usr/bin:/bin TERM=xterm-256color"
           f" skip_global_compinit=1 LANG=C.UTF-8")
    tmux("kill-server")
    tmux("-u", "new-session", "-d", "-s", "t", "-x", "160", "-y", "30", "-c", os.path.dirname(project), f"{env} zsh -i")
    time.sleep(1.5)
    how = {"source": "file", "compiled": "filecode", "eval": "eval"}[mode]
    checks = []

    def check(label, ok):
        checks.append((label, ok, screen().rstrip()))

    keys("cd webapp", "Enter", wait=1.2)
    check("before: the old hint", OLD in screen())
    keys("clear", "Enter")
    keys(STATE, "Enter")
    check("before: loaded once", f"how={how} loads=1 precmd=1 preexec=1 redraw=1 up=history-beginning-search-backward tab=usual-accept" in screen())

    # The upgrade: brew writes the new plugin (and binary) under the open shell.
    now = time.time() + 2
    write(plugin, COUNT + source.replace("rule='╰─ '", "rule='╰═ '"), now, mode == "compiled")
    if mode == "eval":
        os.utime(usual, (now, now))
    keys("clear", "Enter", wait=1.2)
    s = screen()
    check("next prompt: the new hint", NEW in s and OLD not in s)
    keys("Down")
    check("next prompt: ↓ steps", "2/" in screen())
    keys("Tab")
    check("next prompt: Tab fills the line", "pnpm build" in screen().strip().splitlines()[-1] and NEW not in screen())
    keys("C-u", "Enter", "Enter", "Enter", "clear", "Enter")
    keys(STATE, "Enter")
    check("reloaded once, nothing doubled, ↑ still yours",
          f"how={how} loads=2 precmd=1 preexec=1 redraw=1 up=history-beginning-search-backward tab=usual-accept" in screen())
    tmux("kill-server")
    return checks


fail = False
for mode in ("source", "compiled", "eval"):
    for label, ok, shot in run(mode):
        print(f"{'ok  ' if ok else 'FAIL'}  {mode}: {label}")
        if not ok:
            fail = True
            print("\n".join("      |" + l for l in shot.splitlines()[-12:]))
shutil.rmtree(work, ignore_errors=True)
raise SystemExit(1 if fail else 0)
