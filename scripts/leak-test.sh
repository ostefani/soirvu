#!/bin/sh
# soirvu plaintext-leak test (DESIGN v6, §10). macOS, Linux,
# Windows (under Git Bash).
#
# Idea: put two unique canary strings into real entries, use
# the app normally (search hits the FTS path), lock, quit —
# then scan every place plaintext could escape to. If a canary
# is found anywhere outside the encrypted database's
# ciphertext, the build fails: the threat model is broken,
# not the test.
#
# Scanned: the vault folder itself (database, WAL, header —
# SQLCipher encrypts all of it, this proves it), plus the
# per-OS root list next to this script
# (leak-roots.<os>.txt): webview data stores, app config and
# cache folders, crash dumps, temp directories. Canary text
# in an attachment's bytes is covered by the vault scan.
#
# Usage:
#   scripts/leak-test.sh <vault-folder>
# The vault folder is an argument because paths never pass
# through the app UI for this; use a scratch vault (create
# one with `soirvu-cli create <dir>` and open it in the app),
# or a test vault whose canary entries you delete afterwards
# — deletion is a scrubbed tombstone, so they leave nothing.
#
# Exit codes: 0 = clean, 1 = canary found (leak), 2 = bad setup.
# Directory overrides for testing the script itself:
#   LEAK_ROOTS_FILE, LEAK_TMP_DIRS (colon-separated),
#   LEAK_CANARY_SUFFIX (fixed canary suffix instead of random).

set -u

BUNDLE_ID="dev.ostefani.soirvu"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

case "$(uname -s)" in
    Darwin) OS=darwin; QUIT_HINT="Cmd-Q" ;;
    Linux) OS=linux; QUIT_HINT="closing the window and quitting" ;;
    MINGW* | MSYS* | CYGWIN*) OS=windows; QUIT_HINT="quitting from the tray/taskbar" ;;
    *) echo "unsupported OS: $(uname -s)" >&2; exit 2 ;;
esac

VAULT_DIR="${1:-${VAULT_DIR:-}}"
if [ -z "$VAULT_DIR" ] || [ ! -d "$VAULT_DIR" ]; then
    echo "usage: $0 <vault-folder>" >&2
    exit 2
fi

SUFFIX="${LEAK_CANARY_SUFFIX:-$(od -An -N4 -tx1 /dev/urandom | tr -d ' \n')}"
CANARY_LATIN="SOIRVU-CANARY-$SUFFIX"
CANARY_CYRILLIC="канарейка-$SUFFIX"

cat <<EOF

Plaintext-leak test ($OS). Two canaries for this run:

  Latin:    $CANARY_LATIN
  Cyrillic: $CANARY_CYRILLIC

In the app, with the vault at $VAULT_DIR unlocked:

  1. New entry. Title: the Latin canary. Body: a line of normal
     text with the Latin canary in the middle. Add a tag with
     the Latin canary in it. Wait for "Saved".
  2. New entry. Body: normal text with the Cyrillic canary in
     it. Wait for "Saved".
  3. Search for the first word of the Latin canary, then for
     the Cyrillic canary's first word. Both must find their
     entry (this exercises the FTS path).
  4. Lock the vault, then QUIT the app completely ($QUIT_HINT).

Then come back here and press Enter.
EOF
# shellcheck disable=SC2034
read -r _

# The scan is only meaningful with the app fully quit: a
# running webview may still hold files open or flush on exit.
app_running() {
    if [ "$OS" = windows ]; then
        tasklist 2>/dev/null | grep -qi "soirvu.exe"
    else
        pgrep -x "soirvu" >/dev/null 2>&1 || pgrep -x "Soirvu" >/dev/null 2>&1
    fi
}
while app_running; do
    echo "The app still seems to be running. Quit it completely, then press Enter."
    read -r _
done

fail=0
scan_dir() {
    # $1 = label, $2 = dir. A missing dir is fine (nothing stored).
    [ -d "$2" ] || return 0
    for canary in "$CANARY_LATIN" "$CANARY_CYRILLIC"; do
        hits="$(grep -r -a -F -l -- "$canary" "$2" 2>/dev/null || true)"
        if [ -n "$hits" ]; then
            fail=1
            echo "LEAK: canary found in $1 ($2):"
            echo "$hits" | sed 's/^/  /'
        fi
    done
}

scan_dir "the vault folder" "$VAULT_DIR"

# Per-OS roots: one directory per line in leak-roots.<os>.txt,
# with __HOME__, __BUNDLE_ID__, __APPDATA__ and
# __LOCALAPPDATA__ expanded here.
ROOTS_FILE="${LEAK_ROOTS_FILE:-$SCRIPT_DIR/leak-roots.$OS.txt}"
if [ ! -f "$ROOTS_FILE" ]; then
    echo "missing roots file: $ROOTS_FILE" >&2
    exit 2
fi
while IFS= read -r line; do
    case "$line" in "" | \#*) continue ;; esac
    dir="$(printf '%s' "$line" | sed \
        -e "s|__HOME__|$HOME|g" \
        -e "s|__BUNDLE_ID__|$BUNDLE_ID|g" \
        -e "s|__APPDATA__|${APPDATA:-}|g" \
        -e "s|__LOCALAPPDATA__|${LOCALAPPDATA:-}|g")"
    scan_dir "an app location" "$dir"
done <"$ROOTS_FILE"

# macOS logs: only files whose names point at this app, per
# §10's "logs" — scanning every app's logs would blame
# others' text. (On Linux the state dir is already a root;
# on Windows the crash-dump folder is.)
if [ "$OS" = darwin ] && [ -d "$HOME/Library/Logs" ]; then
    find "$HOME/Library/Logs" -iname "*soirvu*" -type f 2>/dev/null | while IFS= read -r f; do
        for canary in "$CANARY_LATIN" "$CANARY_CYRILLIC"; do
            if grep -a -F -q -- "$canary" "$f" 2>/dev/null; then
                echo "LEAK: canary found in a log file: $f"
                echo "__LEAK_FOUND__"
            fi
        done
    done > /tmp/soirvu-leak-logs.$$.txt
    if grep -q "__LEAK_FOUND__" /tmp/soirvu-leak-logs.$$.txt; then
        fail=1
        grep -v "__LEAK_FOUND__" /tmp/soirvu-leak-logs.$$.txt
    fi
    rm -f /tmp/soirvu-leak-logs.$$.txt
fi

# Temp directories (§10). Other apps' files cannot contain
# our canary, so a full scan is safe to blame.
if [ "$OS" = darwin ]; then
    DEFAULT_TMP="${TMPDIR:-/private/tmp}:/private/tmp"
elif [ "$OS" = windows ]; then
    DEFAULT_TMP="${TEMP:-/tmp}"
else
    DEFAULT_TMP="${TMPDIR:-/tmp}:/tmp"
fi
OLD_IFS="$IFS"; IFS=":"
for d in ${LEAK_TMP_DIRS:-$DEFAULT_TMP}; do
    IFS="$OLD_IFS"
    scan_dir "a temp directory" "$d"
    IFS=":"
done
IFS="$OLD_IFS"

if [ "$fail" -ne 0 ]; then
    echo ""
    echo "FAIL: plaintext left the vault. Do not ship this build."
    exit 1
fi
echo ""
echo "PASS: no canary found outside the encrypted database."
exit 0
