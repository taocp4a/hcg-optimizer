#!/usr/bin/env bash
# Builds the static GitHub Pages site into dist/.
# Requires wasm-bindgen-cli matching the wasm-bindgen version in Cargo.toml:
#   cargo install wasm-bindgen-cli --version 0.2.100 --locked
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

want="$(grep -E '^wasm-bindgen *=' Cargo.toml | sed -E 's/.*"=?([0-9.]+)".*/\1/')"
have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)"
[[ "$have" == "$want" ]] || {
  echo "wasm-bindgen-cli $want is required (found: ${have:-none})" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
}

cargo build -p hcg-wasm --target wasm32-unknown-unknown --release --locked
rm -rf dist && mkdir -p dist/pkg
wasm-bindgen --target web --no-typescript --out-dir dist/pkg \
  target/wasm32-unknown-unknown/release/hcg_wasm.wasm
cp -r web/. dist/
touch dist/.nojekyll
echo "dist/ is ready. Preview locally: python3 -m http.server -d dist 8000"
