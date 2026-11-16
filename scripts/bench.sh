#!/usr/bin/env bash
# scripts/bench.sh [corpus]   (needs hyperfine; root to drop the disk cache)
set -euo pipefail
cd "$(dirname "$0")/.."
corpus=${1:-}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cargo build --quiet --release && cp target/release/usual "$work/usual"
export PATH="$work:$PATH" XDG_CACHE_HOME="$work/cache" XDG_STATE_HOME="$work/state"
export skip_global_compinit=1   # Debian's /etc/zsh/zshrc would run compinit, and may stop to ask a question

if [[ -z $corpus ]]; then
  corpus=$work/corpus
  mkdir -p "$corpus/app/.git"
  printf '{"scripts":{"dev":"vite","test":"vitest","build":"vite build"},"devDependencies":{"vite":"5"}}' >"$corpus/app/package.json"
  printf '## Development\n```\npnpm install\npnpm dev\n```\n' >"$corpus/app/README.md"
fi
corpus=$(cd "$corpus" && pwd)
big=$(ls -S "$corpus"/*/README.md 2>/dev/null | sed -n 1p || true)
big=${big%/README.md}
big=${big:-$(ls -d "$corpus"/* | sed -n 1p)}

echo "== Per project, with and without the cache"
USUAL_CORPUS=$corpus cargo test --quiet --release corpus_report -- --nocapture 2>/dev/null | sed -n 3,8p

if command -v hyperfine >/dev/null; then
  echo; echo "== Process: spawn, start, one call (project with the biggest README: ${big##*/})"
  usual "$big" >/dev/null
  hyperfine -N -w 20 -m 200 --style basic -n spawn 'true' -n start 'usual version' \
    -n cached "usual $big" -n fresh "env XDG_CACHE_HOME=/nonexistent usual $big" 2>&1 |
    grep -E '^Benchmark|Time \(mean'
  if [[ -w /proc/sys/vm/drop_caches ]]; then
    echo; echo "== Cold disk (first cd after a reboot)"
    hyperfine -N -m 10 --style basic --prepare 'sh -c "sync; echo 3 > /proc/sys/vm/drop_caches"' \
      -n cold-cached "usual $big" -n cold-fresh "env XDG_CACHE_HOME=/nonexistent usual $big" 2>&1 |
      grep -E '^Benchmark|Time \(mean'
  fi
fi

if command -v zsh >/dev/null; then
  echo; echo "== zsh"
  zdot=$work/zdot && mkdir -p "$zdot" && : >"$zdot/.zshrc"
  zdot2=$work/zdot2 && mkdir -p "$zdot2" && echo 'eval "$(usual init zsh)"' >"$zdot2/.zshrc"
  zdot3=$work/zdot3 && mkdir -p "$zdot3" && echo "source $PWD/usual.plugin.zsh" >"$zdot3/.zshrc"
  if command -v hyperfine >/dev/null; then
    hyperfine -N -w 5 -m 50 --style basic -n 'zsh startup' "env ZDOTDIR=$zdot zsh -i -c exit" \
      -n 'zsh startup + eval usual init' "env ZDOTDIR=$zdot2 zsh -i -c exit" \
      -n 'zsh startup + source plugin' "env ZDOTDIR=$zdot3 zsh -i -c exit" 2>&1 | grep -E '^Benchmark|Time \(mean'
  fi
  (cd "$big" && ZDOTDIR=$zdot2 zsh -i -c '
    zmodload zsh/datetime
    t() { local n=$1 s=$EPOCHREALTIME; shift; repeat $n "$@"; printf "  %-44s %7.1f µs\n" "$*" $(( (EPOCHREALTIME - s) * 1e6 / n )); }
    BUFFER= POSTDISPLAY= region_highlight=() COLUMNS=100 _usual_on=1
    t 1000 _usual_draw
    BUFFER=x; t 1000 _usual_draw
    BUFFER=zz CURSOR=2 _usual_typing=1
    t 1000 eval "_usual_typed=; _usual_draw"
    BUFFER=${_usual_cmds[1][1,2]}
    t 1000 eval "_usual_typed=; _usual_draw"
    BUFFER=x
    t 20 eval "add-zsh-hook precmd _usual_bind; _usual_bind"
    _usual_pwd=; _usual_precmd; sleep 1.1                # (stamps taken in the second a file changed never match)
    t 20 eval "_usual_pwd=; _usual_precmd"
    t 20 eval "_usual_pwd=; command rm -f $_usual_hints$PWD/.hint; _usual_precmd"
    t 1000 _usual_precmd
    t 1000 _usual_preexec "pnpm dev --port 3000"
  ' 2>&1 | sed 's/_usual_//')
fi
