#!/bin/sh
# curl -fsSL https://raw.githubusercontent.com/karlcode/usual/main/install.sh | sh
# Optional: USUAL_INSTALL_DIR=/somewhere/bin USUAL_INSTALL_VERSION=0.1.0
set -eu

repo=karlcode/usual
dir=${USUAL_INSTALL_DIR:-$HOME/.local/bin}
zshrc=${ZDOTDIR:-$HOME}/.zshrc

say() { printf '%s\n' "$*"; }
die() { printf 'usual: %s\n' "$*" >&2; exit 1; }

case $(uname -s) in
  Darwin) os=darwin ;;
  Linux) os=linux ;;
  *) die "no build for $(uname -s); build it from source: https://github.com/$repo" ;;
esac
case $(uname -m) in
  x86_64 | amd64) arch=amd64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) die "no build for $(uname -m); build it from source: https://github.com/$repo" ;;
esac

if [ -n "${USUAL_INSTALL_VERSION:-}" ]; then
  base=https://github.com/$repo/releases/download/v${USUAL_INSTALL_VERSION#v}
else
  base=https://github.com/$repo/releases/latest/download
fi
base=${USUAL_INSTALL_URL:-$base}   # for testing a release before it's published
file=usual_${os}_${arch}.tar.gz

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
say "Downloading usual for $os/$arch..."
curl -fsSL -o "$tmp/$file" "$base/$file" || die "couldn't download $base/$file"
curl -fsSL -o "$tmp/checksums.txt" "$base/checksums.txt" || die "couldn't download the checksums"

want=$(awk -v f="$file" '$2 == f { print $1 }' "$tmp/checksums.txt")
if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$tmp/$file" | awk '{ print $1 }')
else
  got=$(shasum -a 256 "$tmp/$file" | awk '{ print $1 }')
fi
[ -n "$want" ] && [ "$want" = "$got" ] || die "the download doesn't match its checksum; nothing was installed"

tar -xzf "$tmp/$file" -C "$tmp"
mkdir -p "$dir"
mv -f "$tmp/usual" "$dir/usual"
chmod 755 "$dir/usual"
say "Installed $dir/usual"

# A file to source: each new shell then skips starting a process for `usual init zsh` (3-4 ms),
# and compiled it skips parsing too (1-2 ms). zsh ignores the .zwc once the file is newer, or after a zsh upgrade.
data=${XDG_DATA_HOME:-$HOME/.local/share}/usual
plugin=$data/usual.plugin.zsh
mkdir -p "$data"
rm -f "$plugin.zwc"
"$dir/usual" init zsh >"$plugin"
if command -v zsh >/dev/null 2>&1; then
  zsh -fc 'zcompile -- "$1"' zsh "$plugin" 2>/dev/null || rm -f "$plugin.zwc"
fi
say "Installed $plugin"

home() { case $1 in "$HOME"/*) printf '%s' "\$HOME/${1#"$HOME"/}" ;; *) printf '%s' "$1" ;; esac; }
if grep -q 'usual\.plugin\.zsh' "$zshrc" 2>/dev/null; then
  say "$zshrc already loads it."
elif grep -q 'usual init zsh' "$zshrc" 2>/dev/null; then
  say "$zshrc already loads it. New shells start faster if you replace its eval \"\$(usual init zsh)\" line with:"
  say "  source \"$(home "$plugin")\""
else
  {
    printf '\n# usual: the next command for each project, on the prompt\n'
    case ":$PATH:" in
      *":$dir:"*) ;;
      *) printf 'export PATH="%s:$PATH"\n' "$(home "$dir")" ;;
    esac
    printf 'source "%s"\n' "$(home "$plugin")"
  } >>"$zshrc"
  say "Added it to $zshrc."
fi
say "Open a new terminal and cd into a project."
