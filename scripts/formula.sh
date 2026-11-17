#!/usr/bin/env bash
# scripts/formula.sh VERSION CHECKSUMS [BASE_URL] > Formula/usual.rb
set -euo pipefail
version=$1 checksums=$2
base=${3:-https://github.com/${GITHUB_REPOSITORY:-karlcode/usual}/releases/download/v$version}

sha() {
  local sum
  sum=$(awk -v f="usual_$1.tar.gz" '$2 == f { print $1 }' "$checksums")
  [[ -n $sum ]] || { echo "no checksum for $1 in $checksums" >&2; exit 1; }
  echo "$sum"
}

cat <<FORMULA
class Usual < Formula
  desc "Shows a project's likely next command on the zsh prompt"
  homepage "https://github.com/${GITHUB_REPOSITORY:-karlcode/usual}"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
      url "$base/usual_darwin_arm64.tar.gz"
      sha256 "$(sha darwin_arm64)"
    end
    on_intel do
      url "$base/usual_darwin_amd64.tar.gz"
      sha256 "$(sha darwin_amd64)"
    end
  end

  on_linux do
    on_arm do
      url "$base/usual_linux_arm64.tar.gz"
      sha256 "$(sha linux_arm64)"
    end
    on_intel do
      url "$base/usual_linux_amd64.tar.gz"
      sha256 "$(sha linux_amd64)"
    end
  end

  def install
    bin.install "usual"
    # Sourced, it saves each new shell a process; compiled, a parse too. zsh ignores a stale .zwc.
    (pkgshare/"usual.plugin.zsh").write Utils.safe_popen_read(bin/"usual", "init", "zsh")
    system "zsh", "-fc", "zcompile -- #{pkgshare}/usual.plugin.zsh" if which("zsh")
  end

  def caveats
    <<~EOS
      Add this line to the end of ~/.zshrc, then open a new terminal:
        source #{opt_pkgshare}/usual.plugin.zsh
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/usual version")
    assert_match "usual-accept", shell_output("#{bin}/usual init zsh")
    assert_match "usual-accept", (pkgshare/"usual.plugin.zsh").read
  end
end
FORMULA
