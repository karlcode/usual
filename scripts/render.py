#!/usr/bin/env python3
"""scripts/render.py: in tmux, checks the hint never wraps or falls out of line."""
import itertools, os, re, shutil, subprocess, sys, tempfile, time

repo = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
work = tempfile.mkdtemp(prefix="usual-render-")
bin_dir = os.path.join(work, "bin")
os.makedirs(bin_dir)
subprocess.run(["cargo", "build", "--quiet", "--release"], cwd=repo, check=True)
shutil.copy(os.path.join(repo, "target", "release", "usual"), os.path.join(bin_dir, "usual"))
open(os.path.join(bin_dir, "pnpm"), "w").write("#!/bin/sh\n")
os.chmod(os.path.join(bin_dir, "pnpm"), 0o755)

projects = {
    "plain": '{"scripts":{"dev":"vite","build":"vite build","test":"vitest","lint":"eslint ."}}',
    "messy": '{"scripts":{'
             '"dev":"NODE_OPTIONS=--max-old-space-size=8192 cross-env NEXT_TELEMETRY_DISABLED=1 next dev --turbo --port 3000 --hostname 0.0.0.0",'
             '"build:everything-in-the-whole-monorepo":"turbo run build --filter=@acme/web --filter=@acme/api --filter=@acme/docs --concurrency=20",'
             '"test":"vitest run --coverage 测试覆盖率报告 🚀 and a long tail of words to make it wrap somewhere",'
             '"lint":"eslint . --max-warnings 0 --cache --cache-location node_modules/.cache/eslint/"}}',
}
deep = os.path.join(work, "a-rather-long", "folder-path-that", "people-really-have", "in-their-home")
for name, pkg in projects.items():
    p = os.path.join(deep, name)
    os.makedirs(os.path.join(p, ".git"))
    os.makedirs(os.path.join(p, "node_modules"))
    open(os.path.join(p, "package.json"), "w").write(pkg)
    open(os.path.join(p, "pnpm-lock.yaml"), "w").write("")
old = time.time() - 3600
for root, dirs, files in os.walk(work):
    for f in dirs + files:
        os.utime(os.path.join(root, f), (old, old))

prompts = {
    "short": ("PS1='%1~ ❯ '", 0, True),
    "long path": ("PS1='%~ ❯ '", 0, True),
    "two-line": ("PS1=$'%~\\n❯ '", 1, True),
    "emoji": ("PS1='🚀 %1~ ❯ '", 0, True),
    "$(...)": ("setopt promptsubst; PS1='$(print -n λ) %1~ ❯ '", 0, False),
    "RPROMPT": ("PS1='%1~ ❯ '; RPROMPT='[%D{%H:%M}]'", 0, True),
}
widths = [40, 60, 80, 120]
locales = ["C.UTF-8", "C"]


def tmux(*a):
    return subprocess.run(["tmux", "-L", "render", *a], capture_output=True, text=True).stdout


def cols(s):
    import unicodedata
    return sum(2 if unicodedata.east_asian_width(c) in "WF" or ord(c) >= 0x1F000 else 1 for c in s)


def first_col(line, cmd):
    i = line.find(cmd)
    return -1 if i < 0 else cols(line[:i])


def desc_col(line, start):
    rest = line[start:]
    i = rest.find("  ")
    if i < 0:
        return None
    j = i
    while j < len(rest) and rest[j] == " ":
        j += 1
    return cols(line[:start + j]) if j < len(rest) else None


fails, shots, total = [], [], 0
for (proj, (pname, (rc, extra, knowable)), width, loc) in itertools.product(projects, prompts.items(), widths, locales):
    if loc == "C" and pname == "emoji":
        continue
    total += 1
    home = tempfile.mkdtemp(dir=work)
    open(os.path.join(home, ".zshrc"), "w").write(rc + f"\nsource {repo}/usual.plugin.zsh\n")
    # Debian's /etc/zsh/zshrc runs compinit otherwise, which can swallow the first key.
    env = (f"env -i HOME={home} ZDOTDIR={home} PATH={bin_dir}:/usr/bin:/bin TERM=xterm-256color skip_global_compinit=1 "
           f"LANG={loc} LC_ALL={loc} XDG_CACHE_HOME={work}/cache XDG_STATE_HOME={home}/state")
    tmux("kill-server")
    tmux("-u", "new-session", "-d", "-s", "t", "-x", str(width), "-y", "24", "-c", deep, f"{env} zsh -i")
    time.sleep(1.2)
    tmux("send-keys", "-t", "t", f"cd {proj}", "Enter")
    time.sleep(1.0)
    screen = tmux("capture-pane", "-t", "t", "-p").rstrip("\n").split("\n")
    tmux("send-keys", "-t", "t", "p", "n")
    time.sleep(0.8)
    typed = tmux("capture-pane", "-t", "t", "-p").rstrip("\n").split("\n")
    tmux("kill-server")
    while screen and not screen[-1].strip():
        screen.pop()
    while typed and not typed[-1].strip():
        typed.pop()
    label = f"{proj:5} | {pname:9} | {width:3} cols | {loc}"
    shots.append(f"--- {label}\n" + "\n".join(screen))

    problems = []
    rows, footer = screen[-5:-1], screen[-1]
    if any(not l.lstrip().startswith("pnpm") for l in rows[1:]) or "pnpm" not in rows[0] \
            or "/4" not in footer:
        problems.append("a line wrapped")
    elif first_col(footer, footer.lstrip()[:1]) != first_col(rows[1], "pnpm"):
        problems.append("the footer doesn't line up with the rows")
    cmd_cols = [first_col(l, "pnpm") for l in rows]
    if -1 in cmd_cols:
        pass
    elif knowable and loc != "C" and pname != "long path" and len(set(cmd_cols)) != 1:
        problems.append(f"commands start at columns {cmd_cols}")
    elif len(set(cmd_cols[1:])) != 1:
        problems.append(f"rows start at columns {cmd_cols[1:]}")
    dcols = {desc_col(l, c) for l, c in zip(rows[1:], cmd_cols[1:]) if c >= 0} - {None}
    if len(dcols) > 1:
        problems.append(f"descriptions start at columns {sorted(dcols)}")
    if problems:
        fails.append(f"FAIL {label}: " + "; ".join(problems) + "\n" + "\n".join("    |" + l for l in screen[-6:]))

    shots.append(f"--- {label} | typing\n" + "\n".join(typed))
    problems = []
    rows, footer, line = typed[-4:-1], typed[-1], typed[-5] if len(typed) >= 5 else ""
    line = re.sub(r"\s+\[\d\d:\d\d\]\s*$", "", line)
    if any(not l.lstrip().startswith("pnpm") for l in rows) or "4 matches" not in footer or not line.rstrip().endswith("pn"):
        problems.append("a line wrapped, or the list is missing")
    else:
        cols_ = [first_col(l, "pnpm") for l in rows]
        if len(set(cols_)) != 1 or first_col(footer, footer.lstrip()[:1]) != cols_[0]:
            problems.append(f"rows start at columns {cols_}")
        elif knowable and loc != "C" and pname != "long path" and cols(line.rstrip()[:-2]) != cols_[0]:
            problems.append(f"rows start at column {cols_[0]}, the line at {cols(line.rstrip()[:-2])}")
        dcols = {desc_col(l, c) for l, c in zip(rows, cols_)} - {None}
        if len(dcols) > 1:
            problems.append(f"descriptions start at columns {sorted(dcols)}")
    if problems:
        total += 1
        fails.append(f"FAIL {label} | typing: " + "; ".join(problems) + "\n" + "\n".join("    |" + l for l in typed[-6:]))
    else:
        total += 1

out = os.path.join(work, "screens.txt")
open(out, "w").write("\n\n".join(shots))
print("\n".join(fails))
print(f"{total - len(fails)}/{total} render cleanly. Screens: {out}")
sys.exit(1 if fails else 0)
