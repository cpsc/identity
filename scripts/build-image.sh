#!/usr/bin/env bash
# Builds Genkan's container image for this machine's architecture: a static musl binary in an
# otherwise empty image (Containerfile). Run on Linux inside `nix develop`; needs docker.
#   scripts/build-image.sh genkan:dev
set -euo pipefail

tag="${1:?usage: build-image.sh <tag>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
target="$(uname -m | sed -e 's/^arm64$/aarch64/')-unknown-linux-musl"

cargo build --release --locked --target "$target" -p genkan --manifest-path "$root/Cargo.toml"
context="$root/target/image"
rm -rf "$context"
mkdir -p "$context"
cp "$root/target/$target/release/genkan" "$context/genkan"
cp "${GENKAN_CA_BUNDLE:?run inside nix develop}" "$context/ca-certificates.crt"
docker build --file "$root/Containerfile" --tag "$tag" "$context"
size=$(docker image inspect --format '{{.Size}}' "$tag")
echo "image $tag: $size bytes"
