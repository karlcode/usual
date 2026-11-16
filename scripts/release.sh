#!/usr/bin/env bash
# scripts/release.sh VERSION   (0.1.0; needs the four rust targets, zig and cargo-zigbuild)
set -euo pipefail
version=${1:?usage: scripts/release.sh VERSION}
cd "$(dirname "$0")/.."
rm -rf dist && mkdir -p dist
export USUAL_VERSION=$version
export MACOSX_DEPLOYMENT_TARGET=11.0 # Big Sur and later
for target in x86_64-unknown-linux-musl:linux_amd64 aarch64-unknown-linux-musl:linux_arm64 \
  x86_64-apple-darwin:darwin_amd64 aarch64-apple-darwin:darwin_arm64; do
  triple=${target%%:*} name=usual_${target#*:}
  cargo zigbuild --quiet --release --locked --target "$triple"
  mkdir -p "dist/$name"
  cp "target/$triple/release/usual" LICENSE README.md usual.plugin.zsh "dist/$name/"
  tar -czf "dist/$name.tar.gz" -C "dist/$name" .
  rm -rf "dist/${name:?}"
done
(cd dist && sha256sum ./*.tar.gz | sed 's# \./# #' >checksums.txt)
cat dist/checksums.txt
