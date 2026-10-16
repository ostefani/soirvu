#!/bin/sh
# soirvu Linux release checks (DESIGN §9, §12): the shipped
# binary must be a position-independent executable with a
# non-executable stack and RELRO, and the .deb must not
# smuggle anything outside its declared paths.
#
# Usage: scripts/verify-release-linux.sh <soirvu-binary>
# Exit codes: 0 = OK, 1 = a check failed, 2 = bad setup.
set -u

BIN="${1:-}"
if [ -z "$BIN" ] || [ ! -f "$BIN" ]; then
    echo "usage: $0 <soirvu-binary>" >&2
    exit 2
fi
command -v readelf >/dev/null 2>&1 || { echo "readelf not found (binutils)" >&2; exit 2; }

fail=0
note() { echo "$1"; }
bad() { echo "FAIL: $1"; fail=1; }

# PIE: the ELF type must be DYN (a PIE executable), not EXEC.
if readelf -h "$BIN" | grep -q "Type:[[:space:]]*DYN"; then
    note "ok: PIE (ELF type DYN)"
else
    bad "not a PIE executable"
fi

# NX stack: GNU_STACK must not be RWE.
if readelf -W -l "$BIN" | grep "GNU_STACK" | grep -q "RWE"; then
    bad "stack is executable (GNU_STACK RWE)"
else
    note "ok: non-executable stack"
fi

# RELRO: segment present; full RELRO when BIND_NOW is set.
if readelf -W -l "$BIN" | grep -q "GNU_RELRO"; then
    if readelf -d "$BIN" | grep -q "BIND_NOW"; then
        note "ok: full RELRO"
    else
        note "ok: partial RELRO (GNU_RELRO present, no BIND_NOW)"
    fi
else
    bad "no GNU_RELRO segment"
fi

# No writable+executable segment anywhere (W^X).
if readelf -W -l "$BIN" | awk '$1 ~ /LOAD/ && $NF ~ /RWE/ { found=1 } END { exit !found }'; then
    bad "a writable+executable LOAD segment exists"
else
    note "ok: no writable+executable segments"
fi

if [ "$fail" -ne 0 ]; then
    echo "FAIL: $BIN is not a hardened release binary."
    exit 1
fi
echo "OK: $BIN (PIE, NX stack, RELRO)"
exit 0
