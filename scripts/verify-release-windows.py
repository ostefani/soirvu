#!/usr/bin/env python3
"""soirvu Windows release checks (DESIGN §9, §12).

Reads the PE optional header's DllCharacteristics directly
(no dumpbin dependency) and requires the mitigations the
release build must carry: ASLR (dynamic base + high
entropy VA), DEP/NX, and Control Flow Guard instrumentation
(the Guard CF flag in the load-config directory).

Usage: verify-release-windows.py <soirvu.exe>
Exit codes: 0 = OK, 1 = a check failed, 2 = bad setup.
"""

import struct
import sys

IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE = 0x0040
IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA = 0x0020
IMAGE_DLLCHARACTERISTICS_NX_COMPAT = 0x0100
IMAGE_DLLCHARACTERISTICS_GUARD_CF = 0x4000


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: verify-release-windows.py <soirvu.exe>", file=sys.stderr)
        return 2
    try:
        data = open(sys.argv[1], "rb").read()
    except OSError as e:
        print(f"cannot read {sys.argv[1]}: {e}", file=sys.stderr)
        return 2

    if data[:2] != b"MZ":
        print("FAIL: not a PE file (no MZ header)")
        return 1
    (pe_off,) = struct.unpack_from("<I", data, 0x3C)
    if data[pe_off : pe_off + 4] != b"PE\0\0":
        print("FAIL: not a PE file (no PE signature)")
        return 1
    (num_sections,) = struct.unpack_from("<H", data, pe_off + 6)
    (opt_size,) = struct.unpack_from("<H", data, pe_off + 20)
    opt = pe_off + 24
    (magic,) = struct.unpack_from("<H", data, opt)
    if magic != 0x20B:
        print("FAIL: not a PE32+ (64-bit) image")
        return 1
    (dll_chars,) = struct.unpack_from("<H", data, opt + 70)

    fail = False

    def check(flag: int, name: str) -> None:
        nonlocal fail
        if dll_chars & flag:
            print(f"ok: {name}")
        else:
            print(f"FAIL: missing {name}")
            fail = True

    check(IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE, "ASLR (DYNAMIC_BASE)")
    check(IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA, "high-entropy VA")
    check(IMAGE_DLLCHARACTERISTICS_NX_COMPAT, "DEP (NX_COMPAT)")

    # Control Flow Guard: the load-config directory (data
    # directory index 10) must exist and its GuardFlags must
    # carry CF instrumented (0x100) — the DllCharacteristics
    # GUARD_CF bit alone only says the image is CF-aware.
    dd_base = opt + 112
    load_cfg_rva, load_cfg_size = struct.unpack_from("<II", data, dd_base + 10 * 8)
    sections = []
    sec_off = opt + opt_size
    for i in range(num_sections):
        vsize, vaddr, raw_size, raw_ptr = struct.unpack_from(
            "<IIII", data, sec_off + i * 40 + 8
        )
        sections.append((vaddr, max(vsize, raw_size), raw_ptr))

    def rva_to_off(rva: int) -> int | None:
        for vaddr, span, raw_ptr in sections:
            if vaddr <= rva < vaddr + span:
                return raw_ptr + (rva - vaddr)
        return None

    guard_ok = False
    if load_cfg_rva and load_cfg_size >= 0x48:
        off = rva_to_off(load_cfg_rva)
        if off is not None:
            (guard_flags,) = struct.unpack_from("<I", data, off + 0x48)
            guard_ok = bool(guard_flags & 0x100)
    if guard_ok:
        print("ok: Control Flow Guard instrumented")
    else:
        print("FAIL: Control Flow Guard not instrumented")
        fail = True

    if fail:
        print(f"FAIL: {sys.argv[1]} is not a hardened release binary.")
        return 1
    print(f"OK: {sys.argv[1]} (ASLR, DEP, CFG)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
