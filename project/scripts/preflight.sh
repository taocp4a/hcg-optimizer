#!/usr/bin/env bash
# Mandatory checks before opening a PR. CI runs this exact script.
#   usage: project/scripts/preflight.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
fail() { printf '\033[1;31mPREFLIGHT FAIL: %s\033[0m\n' "$*" >&2; exit 1; }

step "1/7 CLAUDE.md == AGENTS.md"
[[ -f CLAUDE.md && -f AGENTS.md ]] || fail "both CLAUDE.md and AGENTS.md must exist"
cmp -s CLAUDE.md AGENTS.md || {
  diff -u CLAUDE.md AGENTS.md || true
  fail "CLAUDE.md and AGENTS.md differ. If you edited one, run 'cp CLAUDE.md AGENTS.md'"
}

step "2/7 layer dependency rules (clean architecture)"
python3 -I "$ROOT/project/scripts/check_layers.py"

step "3/7 cargo fmt --check"
cargo fmt --all -- --check || fail "formatting is off. Run 'cargo fmt --all'"

step "4/7 cargo clippy (warnings = errors)"
cargo clippy --workspace --all-targets --locked -- -D warnings

step "5/7 cargo test"
cargo test --workspace --locked

step "6/7 wasm32 build (hcg-wasm)"
cargo build -p hcg-wasm --target wasm32-unknown-unknown --release --locked

step "7/7 Python reference: roundtrip + reference_summary.json up to date"
"$ROOT/project/scripts/reference_roundtrip.sh"
python3 -I "$ROOT/project/scripts/reference_summary.py" --check

printf '\n\033[1;32mPREFLIGHT OK\033[0m\n'
