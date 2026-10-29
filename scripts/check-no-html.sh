#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Olha Stefanishyna
# The {@html} lint rule (DESIGN v6, Phase 2).
#
# Svelte's {@html} renders a string as markup. Diary text must
# never reach it: entries are rendered as text nodes and <mark>
# segments only (search snippets included — the bridge sends
# plain text with control-char markers, never HTML). One
# {@html} anywhere under src/ fails this check.
#
# The pattern requires whitespace after "@html": that is how
# the tag is always written ({@html value}); prose that merely
# mentions it, like {@html} in a comment, does not match.
set -u
cd "$(dirname "$0")/.."
if grep -rnE --include='*.svelte' --include='*.js' '\{@html[[:space:]]' src/; then
    echo "FAIL: {@html} is forbidden in src/ — render diary text as text." >&2
    exit 1
fi
echo "OK: no {@html} in src/."
