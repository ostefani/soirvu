#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Olha Stefanishyna
# §9 release checks for a built SoirVu.app (macOS, local build):
# - the hardened runtime flag survived ad-hoc signing
# - no get-task-allow (debugger) entitlement
# - no library-validation exemption
# - no dyld environment entitlements
# - the signature is ad-hoc (identity "-"), as designed
set -eu
APP="${1:?usage: verify-release.sh /path/to/SoirVu.app}"
INFO="$(codesign -dv --verbose=4 "$APP" 2>&1)"
echo "$INFO" | grep -q "flags=.*runtime" || {
    echo "FAIL: hardened runtime flag missing"
    exit 1
}
ENT="$(codesign -d --entitlements - "$APP" 2>/dev/null || true)"
for bad in get-task-allow disable-library-validation allow-dyld-environment-variables; do
    case "$ENT" in
        *"$bad"*)
            echo "FAIL: entitlement present: $bad"
            exit 1
            ;;
    esac
done
echo "$INFO" | grep -q "Signature=adhoc" || {
    echo "FAIL: expected an ad-hoc signature"
    exit 1
}
echo "OK: hardened runtime, no exemptions, ad-hoc signature"
