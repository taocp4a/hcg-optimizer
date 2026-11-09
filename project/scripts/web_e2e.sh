#!/usr/bin/env bash
# Builds the site and runs the browser end-to-end test (issue #17) with Playwright + Chromium.
# Needs wasm-bindgen-cli, node, and the playwright package with a Chromium browser.
#   usage: project/scripts/web_e2e.sh [--no-build]
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/target/e2e"
# Find a globally installed playwright if NODE_PATH does not point to one.
export NODE_PATH="${NODE_PATH:-$(npm root -g 2>/dev/null || true)}"
rm -rf "$OUT" && mkdir -p "$OUT"
[[ "${1:-}" == "--no-build" ]] || "$ROOT/project/scripts/build_web.sh" >/dev/null

# Input: speedtest.hcg + speedtest/ as one zip.
python3 -I - "$ROOT/project/samples" "$OUT/speedtest.zip" <<'PY'
import os, sys, zipfile
samples, out = sys.argv[1:]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.write(os.path.join(samples, "speedtest.hcg"), "speedtest.hcg")
    for name in sorted(os.listdir(os.path.join(samples, "speedtest"))):
        z.write(os.path.join(samples, "speedtest", name), "speedtest/" + name)
PY

PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
python3 -m http.server "$PORT" --bind 127.0.0.1 -d "$ROOT/dist" >/dev/null 2>&1 &
SERVER=$!
trap 'kill $SERVER' EXIT
sleep 1

RESULT=$(node "$ROOT/project/scripts/web_e2e.js" "http://127.0.0.1:$PORT/" "$OUT/speedtest.zip" "$OUT" "$ROOT/project/samples/speedtest.hcg")
echo "$RESULT"
SAVED=$(echo "$RESULT" | sed -n 's/^SAVED //p')

# The downloaded zip has the same layout as the CLI output (W5).
python3 -I - "$SAVED" <<'PY'
import sys, zipfile
names = zipfile.ZipFile(sys.argv[1]).namelist()
for expected in ["speedtest_optimized/speedtest.hcg", "speedtest_optimized/점검결과.txt",
                 "speedtest_optimized/speedtest/3F8B560Bbg_1080_noise.jpg"]:
    assert expected in names, f"{expected} missing from {names}"
print(f"ok: downloaded zip has the expected layout ({len(names)} files)")
PY
echo "web e2e OK (screenshots in $OUT)"
