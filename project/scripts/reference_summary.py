#!/usr/bin/env python3
"""Writes project/samples/reference_summary.json from the Python reference parser.

The Rust hcg-format tests compare their own results against this file (contract C6), so it is
the bridge between the reference implementation and the Rust port.

  usage: reference_summary.py           # regenerate the file
         reference_summary.py --check   # fail if the committed file is out of date (preflight)
"""
import json, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, "project", "reference"))
import hcg_parse as H  # noqa: E402

SAMPLES = os.path.join(ROOT, "project", "samples")
OUT = os.path.join(SAMPLES, "reference_summary.json")


def images(layer, page, layer_idx):
    out = []
    for obj_idx, obj in enumerate(layer["objects"]):
        blocks = ([("primary", obj["image"])] if "image" in obj else []) + [
            (f"sequence:{i}", b) for i, b in enumerate(obj.get("images", []))
        ]
        for role, b in blocks:
            if b["size"] <= 0:
                source = {"empty": True}
            elif b["is_path"]:
                source = {"path": b["path"], "folder_file_name": b["subfolder_name"]}
            else:
                source = {"embedded_len": b["embedded_bytes"]}
            out.append({"page": page, "layer": layer_idx, "object": obj_idx, "role": role,
                        "stamp": b["stamp"], **source})
    return out


def summarize(path):
    with open(path, "rb") as f:
        F = H.parse(f.read())
    imgs = images(F["global_layer"], None, 0)
    pages = []
    for p_idx, P in enumerate(F["pages"]):
        hdr = P["hdr"]  # i32 view of the 0x6C page header
        pages.append({
            "width": hdr[3], "height": hdr[4],
            "transition_enabled": hdr[9], "transition_kind": hdr[10], "transition_ms": hdr[11],
            "object_count": sum(L["n_objects"] for L in P["layers"]),
        })
        for l_idx, L in enumerate(P["layers"]):
            imgs += images(L, p_idx, l_idx)
    return {
        "version": F["version"],
        "global_objects": F["global_layer"]["n_objects"],
        "pages": pages,
        "images": imgs,
    }


def build():
    files = sorted(
        os.path.relpath(os.path.join(d, n), SAMPLES).replace(os.sep, "/")
        for d, _, names in os.walk(SAMPLES) for n in names if n.endswith(".hcg")
    )
    return json.dumps({f: summarize(os.path.join(SAMPLES, f)) for f in files},
                      ensure_ascii=False, indent=1, sort_keys=True) + "\n"


if __name__ == "__main__":
    text = build()
    if "--check" in sys.argv:
        try:
            with open(OUT, encoding="utf-8") as f:
                current = f.read()
        except FileNotFoundError:
            current = ""
        if current != text:
            print("reference_summary.json is out of date. Run project/scripts/reference_summary.py",
                  file=sys.stderr)
            sys.exit(1)
        print("reference summary OK")
    else:
        with open(OUT, "w", encoding="utf-8") as f:
            f.write(text)
        print("wrote", os.path.relpath(OUT, ROOT))
