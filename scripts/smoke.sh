#!/usr/bin/env bash
# scripts/smoke.sh path/to/usual
set -euo pipefail
bin=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

"$bin" version
"$bin" init zsh | grep -q usual-accept

mk() { mkdir -p "$(dirname "p/$1")" && printf '%b' "$2" >"p/$1"; }
mk node/package.json '{"scripts":{"dev":"vite","build":"tsc && vite build","test":"vitest","lint":"eslint ."},"devDependencies":{"vite":"5"}}'
mk node/README.md '## Development\n```sh\n$ npm install\n$ npm run dev\n```\n'
mk make/Makefile '.PHONY: test lint\n## Run the tests\ntest:\n\tgo test ./...\nlint: ## Lint\n\tgolangci-lint run\n'
mk just/justfile '# Build it\nbuild:\n    cargo build\ntest:\n    cargo test\n'
mk cargo/Cargo.toml '[package]\nname = "x"\n'
mk cargo/src/main.rs 'fn main() {}'
mk mise/mise.toml '[tasks.dev]\ndescription = "Dev server"\nrun = "vite"\n'
mkdir -p p/node/src bin home
for t in npm make just cargo mise; do printf '#!/bin/sh\n' >"bin/$t" && chmod +x "bin/$t"; done
run() { env -i HOME="$work/home" PATH="$work/bin" XDG_CACHE_HOME="$work/cache" XDG_STATE_HOME="$work/state" "$bin" "$@"; }

fail=0
expect() {
  local dir=$1 got
  shift
  got=$(run "p/$dir")
  if [[ $got != "$(printf '%s\n' "$@")" ]]; then
    printf 'p/%s: expected\n%s\ngot\n%s\n' "$dir" "$(printf '%s\n' "$@")" "$got"
    fail=1
  fi
}
T=$'\t'
node=("npm install${T}dependencies not installed" "npm run dev${T}vite" "npm run build${T}type-check, build"
  "npm test${T}vitest" "npm run lint${T}eslint")
expect node "${node[@]}"
expect node/src "${node[@]}"
expect make "make test${T}Run the tests" "make lint${T}Lint"
expect just "just build${T}Build it" "just test${T}cargo test"
expect cargo "cargo run${T}" "cargo build${T}" "cargo test${T}"
expect mise "mise run dev${T}Dev server"

want=$(printf '%s\n' "$work/p/mise" 1 "mise run dev" "Dev server")
for pass in fresh cached; do
  [[ $(run suggest "$work/p/mise") == "$want" ]] || { echo "suggest ($pass) differs"; fail=1; }
done
run --json p/just | grep -q '"command": "just build"' || { echo "--json differs"; fail=1; }

[[ $fail == 0 ]] && echo "suggests what it should"
exit $fail
