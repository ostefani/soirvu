// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Regression test: a folder scope must reach Rust with the
// snake_case flag the `ScopeInput` struct declares. Sending the
// JS-cased key rejects every scoped read (entry list, calendar,
// search, tags) with `missing field include_descendants` —
// exactly the failure clicking a folder produced.

import { test } from "node:test";
import assert from "node:assert/strict";

import { scopeToWire, searchScopeKey } from "../scope.js";

test("vault-wide scopes pass through as null or kind-only", () => {
    assert.equal(scopeToWire(null), null);
    assert.equal(scopeToWire(undefined), null);
    assert.deepEqual(scopeToWire({ kind: "all" }), null);
    assert.deepEqual(scopeToWire({ kind: "root" }), { kind: "root" });
});

test("a folder scope carries the snake_case subtree flag", () => {
    assert.deepEqual(scopeToWire({ kind: "folder", id: "f1", includeDescendants: false }), {
        kind: "folder",
        id: "f1",
        include_descendants: false,
    });
    assert.deepEqual(scopeToWire({ kind: "folder", id: "f1", includeDescendants: true }), {
        kind: "folder",
        id: "f1",
        include_descendants: true,
    });
    // The flag defaults to the direct scope, like the UI toggle.
    assert.deepEqual(scopeToWire({ kind: "folder", id: "f1" }), {
        kind: "folder",
        id: "f1",
        include_descendants: false,
    });
});

test("the wire shape has no camelCase twin of the flag", () => {
    const wire = scopeToWire({ kind: "folder", id: "f1", includeDescendants: true });
    assert.ok(!("includeDescendants" in wire));
});

test("the search scope key follows the checkbox and the list scope", () => {
    const folder = { kind: "folder", id: "f1", includeDescendants: false };
    // Toggling the checkbox changes the key, so an active search re-runs.
    assert.notEqual(searchScopeKey(false, folder), searchScopeKey(true, folder));
    // Moving between folders — or flipping the subtree flag — too.
    assert.notEqual(
        searchScopeKey(true, folder),
        searchScopeKey(true, { kind: "folder", id: "f2", includeDescendants: false }),
    );
    assert.notEqual(
        searchScopeKey(true, folder),
        searchScopeKey(true, { kind: "folder", id: "f1", includeDescendants: true }),
    );
    // An unchecked box always reads the whole vault, whatever the list shows.
    assert.equal(searchScopeKey(false, folder), searchScopeKey(false, null));
    assert.equal(searchScopeKey(false, folder), "all");
    // The key carries no query text: typing alone must not re-fire it.
    assert.equal(searchScopeKey(true, folder), searchScopeKey(true, { ...folder }));
});
