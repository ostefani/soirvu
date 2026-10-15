#!/bin/sh
# soirvu plaintext-leak test (DESIGN v6, §10). macOS.
#
# Idea: put two unique canary strings into real entries, use the
# app normally (search hits the FTS path), lock, quit — then scan
# every place plaintext could escape to. If a canary is found
# anywhere outside the encrypted database's ciphertext, the
# build fails: the threat model is broken, not the test.
#
# Scanned: the vault folder itself (database, WAL, header —
# SQLCipher encrypts all of it, this proves it), the WKWebView
# data store, the app cache and support folders, app log files,
# and the temp directories.
#
# Attachments join in Phase 4: canary text in an attachment's
# bytes is covered by the same vault-folder scan.
#
# Usage:
#   scripts/leak-test.sh <vault-folder>
# The vault folder is an argument because paths never pass
# through the app UI for this; use a scratch vault (create one
# with `soirvu-cli create <dir>` and open it in the app), or a
# test vault whose canary entries you delete afterwards —
# deletion is a scrubbed tombstone, so they leave nothing.
#
# Exit codes: 0 = clean, 1 = canary found (leak), 2 = bad setup.
# Directory overrides for testing the script itself:
#   LEAK_WEBKIT_DIR, LEAK_CACHES_DIR, LEAK_SUPPORT_DIR,
#   LEAK_LOGS_DIR, LEAK_TMP_DIRS (colon-separated),
#   LEAK_CANARY_SUFFIX (fixed canary suffix instead of random).

set -u

BUNDLE_ID="dev.ostefani.soirvu"

VAULT_DIR="${1:-${VAULT_DIR:-}}"
if [ -z "$VAULT_DIR" ] || [ ! -d "$VAULT_DIR" ]; then
    echo "usage: $0 <vault-folder>" >&2
    exit 2
fi

SUFFIX="${LEAK_CANARY_SUFFIX:-$(od -An -N4 -tx1 /dev/urandom | tr -d ' \n')}"
CANARY_LATIN="SOIRVU-CANARY-$SUFFIX"
CANARY_CYRILLIC="канарейка-$SUFFIX"

cat <<EOF

Plaintext-leak test. Two canaries for this run:

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
  4. Lock the vault, then QUIT the app completely (Cmd-Q).

Then come back here and press Enter.
EOF
# shellcheck disable=SC2034
read -r _

# The scan is only meaningful with the app fully quit: a running
# webview may still hold files open or flush caches on exit.
while pgrep -x "soirvu" >/dev/null 2>&1 || pgrep -x "Soirvu" >/dev/null 2>&1; do
    echo "The app still seems to be running. Quit it (Cmd-Q), then press Enter."
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
scan_dir "the WebKit data store" "${LEAK_WEBKIT_DIR:-$HOME/Library/WebKit/$BUNDLE_ID}"
scan_dir "the app caches" "${LEAK_CACHES_DIR:-$HOME/Library/Caches/$BUNDLE_ID}"
scan_dir "the app support folder" "${LEAK_SUPPORT_DIR:-$HOME/Library/Application Support/$BUNDLE_ID}"
scan_dir "the app registry folder" "${LEAK_SUPPORT_DIR:-$HOME/Library/Application Support}/Soirvu"

# Logs: only files whose names point at this app, per §10's
# "logs" — scanning every app's logs would blame others' text.
LOGS_DIR="${LEAK_LOGS_DIR:-$HOME/Library/Logs}"
if [ -d "$LOGS_DIR" ]; then
    find "$LOGS_DIR" -iname "*soirvu*" -type f 2>/dev/null | while IFS= read -r f; do
        for canary in "$CANARY_LATIN" "$CANARY_CYRILLIC"; do
            if grep -a -F -q -- "$canary" "$f" 2>/dev/null; then
                echo "LEAK: canary found in a log file: $f"
                # the pipe runs this in a subshell; mark via stdout
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

# Temp directories (§10). TMPDIR is per-user on macOS; /private/tmp
# is the shared one. Other apps' files cannot contain our canary,
# so a full scan is safe to blame.
OLD_IFS="$IFS"; IFS=":"
for d in ${LEAK_TMP_DIRS:-${TMPDIR:-/private/tmp}:/private/tmp}; do
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
