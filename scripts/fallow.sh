#!/usr/bin/env bash
# fallow (https://github.com/fallow-rs/fallow) on ui/, at a pinned version. nixpkgs has no
# package for it; its npm packages run no install scripts.
#
#   scripts/fallow.sh dead-code --baseline fallow-baseline.json
#   scripts/fallow.sh audit --base main
set -euo pipefail
cd "$(dirname "$0")/../ui"
exec bunx fallow@3.33.1 "$@"
