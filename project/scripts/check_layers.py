#!/usr/bin/env python3
"""Clean-architecture dependency rule check (run by preflight).

Rules
- Every workspace crate must be assigned a layer in LAYERS below (register new crates here).
- An internal crate may depend only on crates in inner layers: not on the same layer, not outward.
- Domain and application layers must not depend directly on I/O or platform crates (INFRA_CRATES).
"""
import json, subprocess, sys

# Lower number = inner layer.
LAYERS = {
    "hcg-format": 0,    # domain
    "hcg-app": 1,       # application (defines ports)
    "hcg-adapters": 2,  # adapters (implement ports)
    "hcg-cli": 3,       # frontend
    "hcg-wasm": 3,      # frontend
}
LAYER_NAMES = {0: "domain", 1: "application", 2: "adapters", 3: "frontend"}
INFRA_CRATES = {
    "zip", "image", "encoding_rs", "flate2", "png", "jpeg-decoder", "zune-jpeg",
    "wasm-bindgen", "js-sys", "web-sys", "eframe", "egui", "tauri", "clap", "tokio",
}
INNER_MAX = 1  # layers <= this may not use INFRA_CRATES

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"]))
errors = []
members = {p["name"]: p for p in meta["packages"]}

for name in members:
    if name not in LAYERS:
        errors.append(f"{name}: no layer assigned in LAYERS (project/scripts/check_layers.py)")

for name, pkg in members.items():
    if name not in LAYERS:
        continue
    me = LAYERS[name]
    for dep in pkg["dependencies"]:
        if dep["kind"] == "dev":
            continue
        d = dep["name"]
        if d in members:
            if d not in LAYERS:
                continue
            if LAYERS[d] >= me:
                errors.append(f"{name} ({LAYER_NAMES[me]}) -> {d} ({LAYER_NAMES[LAYERS[d]]}): "
                              "dependencies may only point to inner layers")
        elif me <= INNER_MAX and d in INFRA_CRATES:
            errors.append(f"{name} ({LAYER_NAMES[me]}) -> {d}: inner layers may not depend on I/O or platform "
                          "crates. Define a port (trait) and implement it in hcg-adapters")

if errors:
    print("layer rule violations:", *errors, sep="\n  - ", file=sys.stderr)
    sys.exit(1)
print("layers OK:", ", ".join(f"{n}={LAYER_NAMES[l]}" for n, l in sorted(LAYERS.items(), key=lambda x: x[1])))
