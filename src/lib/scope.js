// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Scope conversion helpers: the UI owns selections in JS casing,
// the wire speaks the Rust struct shape.

/**
 * Converts an internal folder selection to the wire scope the
 * scoped commands send. Rust deserializes `ScopeInput` with
 * strict serde (the Tauri camelCase conversion applies only to
 * top-level command arguments, never to nested struct fields),
 * so the subtree flag travels snake_case. A missing flag would
 * reject every scoped read with `missing field`.
 * @param {{ kind: "all" } | { kind: "root" } | { kind: "folder", id: string, includeDescendants?: boolean } | null | undefined} sel
 * @returns {{ kind: string, id?: string, include_descendants?: boolean } | null}
 */
export function scopeToWire(sel) {
    if (sel == null || sel.kind === "all") return null;
    if (sel.kind === "root") return { kind: "root" };
    if (sel.kind === "folder") {
        return {
            kind: "folder",
            id: sel.id,
            include_descendants: sel.includeDescendants === true,
        };
    }
    return null;
}

/**
 * Identity of the scope an active search runs under: the checkbox
 * plus the list scope, never the query text. The diary screen
 * re-runs a visible search when this changes (toggling the box,
 * moving between folders, flipping the subtree flag); typing
 * alone keeps the debounced path and must not re-fire it.
 * @param {boolean} scoped
 * @param {{ kind: string, id?: string, includeDescendants?: boolean } | null} scope
 * @returns {string}
 */
export function searchScopeKey(scoped, scope) {
    if (!scoped || scope == null) return "all";
    if (scope.kind === "folder") {
        return `folder:${scope.id}:${scope.includeDescendants === true}`;
    }
    return scope.kind ?? "all";
}
