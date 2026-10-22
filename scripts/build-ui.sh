#!/usr/bin/env bash
# Builds the sign-in page assets into crates/zeabur-identity-ui/assets (committed; `cargo build` embeds them and
# never needs this toolchain). CI rebuilds and fails if the committed files differ.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/crates/zeabur-identity-ui/assets"

tailwindcss --input "$root/ui/styles/app.css" --output "$out/app.css" --minify
for page in login setup account; do
  bun build "$root/ui/src/$page.ts" --target=browser --format=esm --minify --outfile "$out/$page.js" >/dev/null
done
