#!/bin/sh
# §12 seam rule: OS-specific cfg lives in the platform
# files (platform.rs, macos_lock.rs, keychain.rs). Anywhere
# else it needs a `// platform-ok: <reason>` comment on the
# line directly above — a deliberate exception, visible in
# review, not a habit. lib.rs may declare the platform
# modules under cfg, on the two lines that takes.
#
# Usage: sh scripts/check-platform-cfg.sh [src-dir]
# (defaults to src-tauri/src; exit 1 and print offenders.)
set -u
SRC="${1:-src-tauri/src}"
fail=0
for f in $(grep -rl 'cfg(target_os' "$SRC" --include='*.rs' | sort); do
    case "$f" in
        */platform.rs | */macos_lock.rs | */keychain.rs) continue ;;
    esac
    awk -v file="$f" '
        { lines[NR] = $0 }
        END {
            bad = 0
            for (i = 1; i <= NR; i++) {
                if (lines[i] !~ /cfg\(target_os/) continue
                if (lines[i-1] ~ /platform-ok:/) continue
                if (file ~ /lib\.rs$/ && lines[i+1] ~ /mod (macos_lock|keychain|platform);/) continue
                printf "%s:%d: %s\n", file, i, lines[i]
                bad = 1
            }
            exit bad
        }
    ' "$f" || fail=1
done
if [ "$fail" -ne 0 ]; then
    echo "platform cfg outside the platform files (see scripts/check-platform-cfg.sh)" >&2
    exit 1
fi
echo "platform cfg: OK"
