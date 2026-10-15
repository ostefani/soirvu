#!/bin/sh
# §9/§11: the built frontend bundle must not reference external
# hostnames — the app makes no network requests, ever, and a
# compromised dependency would need a URL to phone home to.
set -eu
DIST="${1:-dist}"
HOSTS="$(grep -rEoh 'https?://[A-Za-z0-9.-]+' "$DIST" | sort -u || true)"
# The legitimate strings: the SVG/XML namespace identifier, and
# the documentation links inside Svelte's and ProseMirror's
# error messages (inert text, never fetched).
ALLOWED="http://www.w3.org https://svelte.dev https://prosemirror.net"
FAIL=0
for h in $HOSTS; do
    case " $ALLOWED " in
        *" $h "*) ;;
        *)
            echo "external host in bundle: $h"
            FAIL=1
            ;;
    esac
done
if [ "$FAIL" -eq 0 ]; then
    echo "OK: no external hosts in $DIST"
fi
exit "$FAIL"
