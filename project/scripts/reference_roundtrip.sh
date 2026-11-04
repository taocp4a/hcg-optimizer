#!/usr/bin/env bash
# Parses and re-writes every .hcg under project/samples with the Python reference implementation
# and checks the bytes are identical. Guards the oracle the Rust port is measured against.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }

fails=0; total=0
while IFS= read -r -d '' f; do
  total=$((total + 1))
  out="$(python3 -I "$ROOT/project/reference/hcg_write.py" "$f")"
  echo "$out"
  [[ "$out" == *IDENTICAL ]] || fails=$((fails + 1))
done < <(find "$ROOT/project/samples" -name '*.hcg' -print0 | sort -z)

(( total > 0 )) || { echo "no sample .hcg files found" >&2; exit 1; }
(( fails == 0 )) || { echo "roundtrip failed: $fails / $total" >&2; exit 1; }
echo "roundtrip OK: $total files"
