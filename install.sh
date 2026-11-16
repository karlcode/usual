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

if grep -q 'usual init zsh' "$zshrc" 2>/dev/null; then
  say "$zshrc already loads it."
else
  shown=$dir
  case $dir in "$HOME"/*) shown="\$HOME/${dir#"$HOME"/}" ;; esac
  {
    printf '\n# usual: the next command for each project, on the prompt\n'
    case ":$PATH:" in
      *":$dir:"*) ;;
      *) printf 'export PATH="%s:$PATH"\n' "$shown" ;;
    esac
    printf 'eval "$(usual init zsh)"\n'
  } >>"$zshrc"
  say "Added it to $zshrc."
fi
say "Open a new terminal and cd into a project."
