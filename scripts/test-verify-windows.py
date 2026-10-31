#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Olha Stefanishyna
"""Self-test for verify-release-windows.py (§9).

Builds a minimal synthetic PE32+ in memory and asserts the verifier
accepts it iff GuardFlags (load-config +0x90) carries CF_INSTRUMENTED.
The +0x48 decoy case pins the v1.0.1 bug: the verifier used to read
ProcessAffinityMask instead of GuardFlags.
"""

import os
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
VERIFY = os.path.join(HERE, "verify-release-windows.py")

DLL_CHARS = 0x0040 | 0x0020 | 0x0100 | 0x4000  # ASLR, HE-VA, NX, GuardCF
CF_INSTRUMENTED = 0x100


def make_pe(*, guard_flags_at_0x90, guard_flags_at_0x48=0):
    dos = bytearray(0x80)
    dos[0:2] = b"MZ"
    struct.pack_into("<I", dos, 0x3C, 0x80)
    coff = struct.pack("<IHHIIIHH", 0x4550, 0x8664, 1, 0, 0, 0, 0xF0, 0x22)
    opt = bytearray(0xF0)
    struct.pack_into("<H", opt, 0, 0x20B)
    struct.pack_into("<H", opt, 70, DLL_CHARS)
    struct.pack_into("<II", opt, 112 + 10 * 8, 0x1000, 0xA0)
    sec = struct.pack(
        "<8sIIIIIIHHI", b".text\x00\x00\x00", 0x200, 0x1000, 0x200, 0x400,
        0, 0, 0, 0, 0x60000020,
    )
    head = bytes(dos) + coff + bytes(opt) + sec
    img = bytearray(0x600)
    img[: len(head)] = head
    struct.pack_into("<I", img, 0x400 + 0x90, guard_flags_at_0x90)
    struct.pack_into("<I", img, 0x400 + 0x48, guard_flags_at_0x48)
    return bytes(img)


def run_verify(pe_bytes):
    with tempfile.NamedTemporaryFile(suffix=".exe", delete=False) as f:
        f.write(pe_bytes)
        path = f.name
    try:
        proc = subprocess.run(
            [sys.executable, VERIFY, path],
            capture_output=True,
            text=True,
        )
        return proc.returncode, proc.stdout
    finally:
        os.unlink(path)


def main():
    cases = [
        ("instrumented exe passes", make_pe(guard_flags_at_0x90=CF_INSTRUMENTED), 0),
        ("plain exe fails", make_pe(guard_flags_at_0x90=0), 1),
        # 0x48 is ProcessAffinityMask, not GuardFlags: a flag there
        # must not fool the verifier.
        ("flag at 0x48 alone still fails",
         make_pe(guard_flags_at_0x90=0, guard_flags_at_0x48=CF_INSTRUMENTED), 1),
    ]
    failed = False
    for name, pe, want in cases:
        got, out = run_verify(pe)
        status = "ok" if got == want else "FAIL"
        if got != want:
            failed = True
        print(f"{status}: {name} (exit {got}, want {want})")
        if got != want:
            print(out)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
