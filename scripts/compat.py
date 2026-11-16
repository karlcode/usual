#!/usr/bin/env python3
"""scripts/compat.py PLUGINS_DIR [TOOLS_DIR]: the hint in real zsh setups, in tmux."""
import os, shutil, subprocess, sys, tempfile, time

HINT = "more  ·"
LIST = "choose"
plugins = os.path.abspath(sys.argv[1])
tools = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else ""
repo = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
work = tempfile.mkdtemp(prefix="usual-compat-")
P = lambda *p: os.path.join(plugins, *p)

bin_dir = os.path.join(work, "bin")
os.makedirs(bin_dir)
subprocess.run(["cargo", "build", "--quiet", "--release"], cwd=repo, check=True)
shutil.copy(os.path.join(repo, "target", "release", "usual"), os.path.join(bin_dir, "usual"))
for tool in ["pnpm"] + ([f for f in ("starship", "atuin") if tools and os.path.exists(os.path.join(tools, f))]):
    src = os.path.join(tools, tool) if tool != "pnpm" else None
    dst = os.path.join(bin_dir, tool)
    if src:
        shutil.copy(src, dst)
    else:
        open(dst, "w").write('#!/bin/sh\necho "ran: $*"\n')
    os.chmod(dst, 0o755)
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

USUAL = 'eval "$(usual init zsh)"'
OMZ = f'export ZSH={P("ohmyzsh")}; ZSH_CUSTOM=$HOME/omz-custom; DISABLE_AUTO_UPDATE=true\n'
P10K = (f'POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true\nsource {P("powerlevel10k","powerlevel10k.zsh-theme")}\n'
        f'source {P("powerlevel10k","config","p10k-lean.zsh")}\n')
P10K_INSTANT = ('if [[ -r "${XDG_CACHE_HOME:-$HOME/.cache}/p10k-instant-prompt-${(%):-%n}.zsh" ]]; then\n'
                '  source "${XDG_CACHE_HOME:-$HOME/.cache}/p10k-instant-prompt-${(%):-%n}.zsh"\nfi\n')
COMPINIT = "autoload -Uz compinit && compinit -u\n"

setups = {
    "plain": "PS1='%1~ ❯ '\n" + USUAL,
    "plain + RPROMPT": "PS1='%1~ ❯ '; RPROMPT='[%T]'\n" + USUAL,
    "vi mode (bindkey -v)": "PS1='%1~ ❯ '; bindkey -v\n" + USUAL,
    "oh-my-zsh robbyrussell": OMZ + "ZSH_THEME=robbyrussell; plugins=(git)\nsource $ZSH/oh-my-zsh.sh\n" + USUAL,
    "oh-my-zsh agnoster": OMZ + "ZSH_THEME=agnoster; plugins=(git)\nsource $ZSH/oh-my-zsh.sh\n" + USUAL,
    "oh-my-zsh + autosuggestions + syntax-highlighting": OMZ
        + "ZSH_THEME=robbyrussell; plugins=(git zsh-autosuggestions zsh-syntax-highlighting)\n"
        + "source $ZSH/oh-my-zsh.sh\n" + USUAL,
    "powerlevel10k (instant prompt)": P10K_INSTANT + P10K + USUAL,
    "pure": f"fpath+=({P('pure')}); autoload -U promptinit; promptinit; prompt pure\n" + USUAL,
    "spaceship": f"source {P('spaceship-prompt','spaceship.zsh')}\n" + USUAL,
    "starship": 'eval "$(starship init zsh)"\n' + USUAL,
    "zsh-autosuggestions (usual after)": "PS1='%1~ ❯ '\n" + f"source {P('zsh-autosuggestions','zsh-autosuggestions.zsh')}\n" + USUAL,
    "zsh-autosuggestions (usual before)": "PS1='%1~ ❯ '\n" + USUAL + f"\nsource {P('zsh-autosuggestions','zsh-autosuggestions.zsh')}",
    "zsh-syntax-highlighting": "PS1='%1~ ❯ '\n" + USUAL + f"\nsource {P('zsh-syntax-highlighting','zsh-syntax-highlighting.zsh')}",
    "fast-syntax-highlighting": "PS1='%1~ ❯ '\n" + USUAL + f"\nsource {P('fast-syntax-highlighting','fast-syntax-highlighting.plugin.zsh')}",
    "zsh-history-substring-search": "PS1='%1~ ❯ '\n" + f"source {P('zsh-history-substring-search','zsh-history-substring-search.zsh')}\n"
        + "bindkey '^[[A' history-substring-search-up; bindkey '^[[B' history-substring-search-down\n" + USUAL,
    "fzf-tab": "PS1='%1~ ❯ '\n" + COMPINIT + f"source {P('fzf-tab','fzf-tab.plugin.zsh')}\n" + USUAL,
    "zsh-vi-mode": "PS1='%1~ ❯ '\n" + f"source {P('zsh-vi-mode','zsh-vi-mode.plugin.zsh')}\n" + USUAL,
    "zsh-autocomplete": f"source {P('zsh-autocomplete','zsh-autocomplete.plugin.zsh')}\nPS1='%1~ ❯ '\n" + USUAL,
    "atuin": "PS1='%1~ ❯ '\n" + 'eval "$(atuin init zsh)"\n' + USUAL,
    "prezto": f"source {P('prezto','init.zsh')}\n" + USUAL,
    "everything: omz + p10k + autosuggestions + fsh + substring-search": P10K_INSTANT + OMZ
        + "ZSH_THEME=''; plugins=(git zsh-autosuggestions)\nsource $ZSH/oh-my-zsh.sh\n" + P10K
        + f"source {P('zsh-history-substring-search','zsh-history-substring-search.zsh')}\n"
        + "bindkey '^[[A' history-substring-search-up; bindkey '^[[B' history-substring-search-down\n"
        + USUAL + f"\nsource {P('fast-syntax-highlighting','fast-syntax-highlighting.plugin.zsh')}",
}
needs = {"oh-my-zsh": "ohmyzsh", "powerlevel10k": "powerlevel10k", "pure": "pure", "spaceship": "spaceship-prompt",
         "zsh-autosuggestions": "zsh-autosuggestions", "zsh-syntax": "zsh-syntax-highlighting",
         "fast-syntax": "fast-syntax-highlighting", "history-substring": "zsh-history-substring-search",
         "fzf-tab": "fzf-tab", "zsh-vi-mode": "zsh-vi-mode", "zsh-autocomplete": "zsh-autocomplete",
         "prezto": "prezto", "everything": "ohmyzsh"}


def tmux(*args):
    return subprocess.run(["tmux", "-L", "compat", *args], capture_output=True, text=True).stdout


def screen():
    return tmux("capture-pane", "-t", "t", "-p")


def keys(*k, wait=0.6):
    tmux("send-keys", "-t", "t", *k)
    time.sleep(wait)


def settle(limit=40):
    last, same, start = None, 0, time.time()
    while time.time() - start < limit:
        time.sleep(0.5)
        now = screen()
        same = same + 1 if now == last and now.strip() else 0
        if same >= 3:
            return
        last = now


def run(name, rc):
    home = os.path.join(work, "home-" + "".join(c if c.isalnum() else "-" for c in name))
    os.makedirs(home)
    if "oh-my-zsh" in name or "everything" in name:
        os.makedirs(os.path.join(home, "omz-custom", "plugins"))
        for p in ("zsh-autosuggestions", "zsh-syntax-highlighting"):
            os.symlink(P(p), os.path.join(home, "omz-custom", "plugins", p))
    if name == "prezto":
        os.symlink(P("prezto"), os.path.join(home, ".zprezto"))
        open(os.path.join(home, ".zpreztorc"), "w").write(
            "zstyle ':prezto:load' pmodule environment terminal editor history directory utility completion prompt\n"
            "zstyle ':prezto:module:prompt' theme sorin\n")
    open(os.path.join(home, ".zshrc"), "w").write(rc + "\n")
    open(os.path.join(home, ".zsh_history"), "w").write(": 1:0;echo from-history\n")
    env = f"env -i HOME={home} ZDOTDIR={home} PATH={bin_dir}:/usr/local/bin:/usr/bin:/bin TERM=xterm-256color skip_global_compinit=1 LANG=C.UTF-8 HISTFILE={home}/.zsh_history SAVEHIST=100 HISTSIZE=100"
    for _ in range(2):
        tmux("kill-server")
        tmux("-u", "new-session", "-d", "-s", "t", "-x", "100", "-y", "30", "-c", os.path.dirname(project), f"{env} zsh -i")
        settle()
    shots, checks = [], []

    def check(label, ok):
        checks.append((label, ok))
        shots.append(f"--- {label}: {'ok' if ok else 'FAIL'}\n" + screen().rstrip())

    keys("cd webapp", "Enter", wait=1.2)
    s = screen()
    check("hint on entering", HINT in s and "pnpm dev" in s and "pnpm build" in s)
    rows = s.splitlines()
    first = next((l for l in rows if "pnpm dev" in l), "")
    second = next((l for l in rows if l.lstrip().startswith("pnpm build")), "")
    aligned.append(name if first and first.index("pnpm dev") == second.find("pnpm build") else None)
    keys("Down")
    check("↓ steps", "2/" in screen())
    # A click sends →, so → must leave the hint a hint.
    keys("Right")
    keys("Down")
    check("→ leaves it alone", HINT in screen() and "3/" in screen())
    keys("Up")
    check("↑ steps back", HINT in screen() and "2/" in screen())
    keys("Tab")
    s = screen()
    check("Tab fills the line", HINT not in s and "pnpm build" in (s.strip().splitlines() or [""])[-1])
    keys("C-u", "cd ..", "Enter", "clear", "Enter", wait=0.6)
    keys("cd webapp", "Enter", wait=1.2)
    keys("C-c", wait=0.8)
    s = screen()
    check("Ctrl-C: the hint on the new prompt", HINT in "\n".join(s.strip().splitlines()[-5:]))
    check("Ctrl-C: nothing left above", s.count(HINT) == 1)
    keys("clear", "Enter", "cd ..", "Enter", "cd webapp", "Enter", wait=1.2)
    keys("Up", wait=1.0)
    check("↑ on entering: the setup's own", HINT not in screen())
    keys("C-c", wait=0.8)
    keys("clear", "Enter", "cd ..", "Enter", "cd webapp", "Enter", wait=1.2)
    keys("e", "c", wait=0.8)
    check("typing hides it", HINT not in screen())
    keys("C-u", "Up", wait=1.0)
    check("↑ afterwards: the setup's own (not the hint)", HINT not in screen())
    keys("C-c", wait=0.8)

    def line():
        return next((l for l in reversed(screen().splitlines()) if l.strip() and "choose" not in l
                     and "accept" not in l and not l.lstrip().startswith(("pnpm ", "› pnpm "))), "")

    def listed(cmd):
        return any(l.lstrip().startswith(cmd) for l in screen().splitlines()) and LIST in screen()

    keys("clear", "Enter", wait=0.6)
    keys("p", "n", "p", "m", "Space", "b", wait=1.0)
    check("typing lists what the line could become", listed("pnpm build") and "pnpm b" in line() and "pnpm build" not in line())
    before = [l for l in screen().splitlines() if "pnpm" in l or LIST in l]
    time.sleep(2.0)
    check("stopping changes nothing", before == [l for l in screen().splitlines() if "pnpm" in l or LIST in l])
    keys("Tab", wait=1.0)
    s = screen()
    check("Tab while typing isn't the list's", "ran:" not in s and "pnpm build" not in line())
    keys("C-c", wait=0.8)
    keys("clear", "Enter", wait=0.6)
    keys("t", "e", wait=1.0)
    check("a task's name finds its command", listed("pnpm test"))
    keys("Down", wait=0.6)
    check("↓ steps into the list", "› pnpm test" in screen() and "accept" in screen())
    keys("Tab", wait=0.8)
    check("then Tab takes it", "pnpm test" in line() and LIST not in screen())
    keys("C-u", "l", "i", wait=1.0)
    keys("Down", "Tab", wait=0.8)
    check("↓ and Tab in one breath", "pnpm lint" in line() and LIST not in screen())
    keys("C-u", "p", "n", wait=0.8)
    keys("BSpace", "BSpace", wait=0.8)
    check("deleting it all clears the list", LIST not in screen())
    keys("p", "n", wait=0.8)
    keys("C-c", wait=0.8)
    check("Ctrl-C while typing leaves nothing", LIST not in screen())
    keys("clear", "Enter", wait=0.6)
    keys("p", "n", wait=0.8)
    keys("Up", wait=1.0)
    check("↑ while typing: the setup's own", "› " not in screen() and "accept" not in screen())
    keys("C-c", wait=0.8)
    keys("clear", "Enter", wait=0.6)
    keys("p", "n", "p", "m", "Space", "b", wait=0.8)
    keys("Enter", wait=1.0)
    s = screen()
    check("Enter runs what you typed, not the hint", "ran: b" in s and "ran: build" not in s and LIST not in s)

    keys("clear", "Enter", wait=0.6)
    keys(f"source {home}/.zshrc", "Enter", wait=1.0)
    settle()
    keys("clear", "Enter", "cd ..", "Enter", "cd webapp", "Enter", wait=1.5)
    s = screen()
    check("after loading .zshrc again: the hint", HINT in s and "pnpm dev" in s)
    keys("Down")
    keys("Tab")
    check("after loading .zshrc again: ↓ and Tab", "pnpm build" in line() and HINT not in screen())
    keys("C-u", wait=0.5)
    keys("t", "e", wait=1.0)
    check("after loading .zshrc again: the list as you type", listed("pnpm test"))
    keys("C-c", wait=0.6)
    tmux("kill-server")
    return checks, shots


report, allshots, aligned = [], [], []
for name, rc in setups.items():
    if os.environ.get("ONLY") and os.environ["ONLY"] not in name:
        continue
    missing = [d for k, d in needs.items() if k in name and not os.path.isdir(P(d))]
    if missing or ("starship" in name and not os.path.exists(os.path.join(bin_dir, "starship"))) or \
            ("atuin" in name and not os.path.exists(os.path.join(bin_dir, "atuin"))):
        report.append(f"skip  {name} (missing {', '.join(missing) or 'binary'})")
        continue
    checks, shots = run(name, rc)
    failed = [label for label, ok in checks if not ok]
    report.append(f"{'ok  ' if not failed else 'FAIL'}  {name}" + (f"   ✗ {'; '.join(failed)}" if failed else ""))
    if failed:
        report[-1] += "\n" + "\n".join("      |" + l for l in next(s for s in shots if s.endswith("FAIL") or ": FAIL\n" in s).splitlines()[:14])
    allshots.append(f"=================== {name}\n" + "\n".join(shots))
    print(report[-1], flush=True)

out = os.path.join(work, "screens.txt")
open(out, "w").write("\n\n".join(allshots))
print("\n".join(r for r in report if r.startswith("skip")))
print("rows line up under the command in:", ", ".join(a for a in aligned if a) or "none")
print("fixed indent instead in:", ", ".join(n for n, a in zip([n for n in setups if n not in [r.split("  ", 1)[1].split(" (missing")[0] for r in report if r.startswith("skip")]], aligned) if not a) or "none")
print("screens:", out)
