#!/usr/bin/env bash
# Time and peak memory of the full pipeline on a 204-page project (issue #13).
# Native: peak RSS. wasm32 (needs node and wasm-bindgen-cli): peak linear memory.
#   usage: project/scripts/bench_memory.sh [pages]
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/target/bench"
PAGES="${1:-204}"
mkdir -p "$OUT"
SAMPLE="$OUT/sunday_$PAGES.zip"
[[ -f "$SAMPLE" ]] || python3 -I "$ROOT/project/scripts/make_bench_sample.py" "$SAMPLE" "$PAGES"
rm -rf "$ROOT/project/reference/__pycache__"

cd "$ROOT/project/bench"
cargo build -q --release --target-dir "$OUT/cargo"
python3 -I - "$OUT/cargo/release/hcg-bench" "$SAMPLE" <<'PY'
import resource, subprocess, sys
r = subprocess.run(sys.argv[1:], capture_output=True, text=True, check=True)
peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss / 1024
print(f"native | {r.stdout.strip()} | peak RSS {peak:.0f} MB")
PY

if command -v node >/dev/null && command -v wasm-bindgen >/dev/null; then
  cargo build -q --release --lib --target wasm32-unknown-unknown --target-dir "$OUT/cargo"
  wasm-bindgen --target nodejs --out-dir "$OUT/pkg" "$OUT/cargo/wasm32-unknown-unknown/release/hcg_bench.wasm"
  node -e '
    const m = require(process.argv[1]);
    const zip = require("fs").readFileSync(process.argv[2]);
    const line = m.run_wasm(zip);
    const peak = m.__wasm.memory.buffer.byteLength / 1048576;
    console.log(`wasm32 | ${line} | peak linear memory ${peak.toFixed(0)} MB (${(peak / (zip.length / 1048576)).toFixed(1)}x input)`);
  ' "$OUT/pkg/hcg_bench.js" "$SAMPLE"
else
  echo "wasm32 | skipped (needs node and wasm-bindgen-cli)"
fi
