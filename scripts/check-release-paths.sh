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

import yaml
wf_text = open(".github/workflows/release.yml").read()
wf = yaml.safe_load(wf_text)
# The RUSTFLAGS step runs on Windows, where the default shell is pwsh:
# `$GITHUB_ENV` there is $null and `>> $null` silently discards, so the
# step must pin `shell: bash` (v1.0.0 shipped a CFG-less exe this way).
for step in wf["jobs"]["build"]["steps"]:
    if step.get("name") == "Control Flow Guard (Windows)":
        if step.get("shell") != "bash":
            sys.exit("FAIL: Control Flow Guard step must set shell: bash (pwsh eats $GITHUB_ENV)")
        break
else:
    sys.exit("FAIL: Control Flow Guard step missing from release.yml")
want = [
    f"target/release/{bname}",
    f"target/release/{bname}.exe",
    f"/usr/bin/{bname}",
    f"dpkg -r {kebab}",
    f"{product}\\{bname}.exe",
    # publish flattens the merged artifact tree before checksumming
    # (v1.0.2 failed on `sha256sum *` over deb/ dmg/ nsis/ dirs).
    "rmdir deb dmg nsis",
    # publish auto-generates the "What's Changed" PR list.
    "generate_release_notes",
]
missing = [w for w in want if w not in wf_text]
if missing:
    sys.exit(f"FAIL: release.yml missing paths for binary {bname!r}: {missing}")
print(f"OK: binary {bname!r}, deb package {kebab!r}, NSIS dir {product!r}")
EOF
