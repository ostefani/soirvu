#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Olha Stefanishyna
# Release-path coherence (§9): the binary name baked into
# .github/workflows/release.yml must match what cargo actually
# builds. v1.0.0's release failed here: cargo built `SoirVu`
# while the workflow verified `target/release/soirvu`.
set -eu
cd "$(dirname "$0")/.."

python3 - <<'EOF'
import json, re, sys, tomllib

manifest = tomllib.load(open("src-tauri/Cargo.toml", "rb"))
bins = manifest.get("bin") or [{"name": manifest["package"]["name"]}]
names = [b["name"] for b in bins]
if len(names) != 1:
    sys.exit(f"FAIL: expected exactly one binary target, found {names}")
(bname,) = names
if bname != bname.lower():
    sys.exit(f"FAIL: binary target {bname!r} must be lowercase (release paths assume it)")

conf = json.load(open("src-tauri/tauri.conf.json"))
product = conf["productName"]
kebab = re.sub(r"(?<!^)(?=[A-Z])", "-", product).lower()

wf = open(".github/workflows/release.yml").read()
want = [
    f"target/release/{bname}",
    f"target/release/{bname}.exe",
    f"/usr/bin/{bname}",
    f"dpkg -r {kebab}",
    f"{product}\\{bname}.exe",
]
missing = [w for w in want if w not in wf]
if missing:
    sys.exit(f"FAIL: release.yml missing paths for binary {bname!r}: {missing}")
print(f"OK: binary {bname!r}, deb package {kebab!r}, NSIS dir {product!r}")
EOF
