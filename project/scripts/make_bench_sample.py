#!/usr/bin/env python3
"""Builds a large benchmark input (issue #13): layered.hcg's pages repeated to PAGES pages, zipped
with the layered/ image folder under church/.

  usage: make_bench_sample.py <output.zip> [pages]   (default 204)
"""
import os, sys, zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, "project", "reference"))
import hcg_parse as H  # noqa: E402
import hcg_write as W  # noqa: E402

out = sys.argv[1]
pages = int(sys.argv[2]) if len(sys.argv) > 2 else 204
samples = os.path.join(ROOT, "project", "samples")
thumbs = []
with open(os.path.join(samples, "layered.hcg"), "rb") as f:
    F = H.parse(f.read(), thumbs)
thumbs = dict(thumbs)
base = F["pages"]
F["pages"] = [base[i % len(base)] for i in range(pages)]
hcg = W.write(F, {i: thumbs[i % len(base)] for i in range(pages)})

os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("church/sunday.hcg", hcg)
    folder = os.path.join(samples, "layered")
    for name in sorted(os.listdir(folder)):
        z.write(os.path.join(folder, name), "church/sunday/" + name)
print(f"{out}: {pages} pages, hcg {len(hcg) / 2**20:.1f} MB, zip {os.path.getsize(out) / 2**20:.1f} MB")
