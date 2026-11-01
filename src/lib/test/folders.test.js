// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Step 7, executable form: the tree nests server order, orphans
// surface at the root, breadcrumbs stop on gaps and cycles, and
// the move exclusion covers a folder plus its whole subtree.

import { test } from "node:test";
import assert from "node:assert/strict";

import { breadcrumb, buildTree, descendantIds, flattenTree } from "../folders.js";

const FOLDERS = [
    { id: "b", parent_id: "a", name: "Health" },
    { id: "a", parent_id: null, name: "Personal" },
    { id: "c", parent_id: "a", name: "Dev" },
    { id: "d", parent_id: "ghost", name: "Orphan" },
];

test("children nest under parents in server order", () => {
    const { roots, recovered } = buildTree(FOLDERS);
    assert.equal(roots.length, 1);
    assert.equal(roots[0].folder.id, "a");
    assert.deepEqual(
        roots[0].children.map((n) => n.folder.id),
        ["b", "c"],
    );
    assert.deepEqual(
        recovered.map((n) => n.folder.id),
        ["d"],
    );
});

test("flatten walks pre-order with depth and skips collapsed subtrees", () => {
    const { roots } = buildTree(FOLDERS);
    const flat = flattenTree(roots, new Set(["a"]));
    assert.deepEqual(
        flat.map((r) => [r.node.folder.id, r.depth]),
        [["a", 0]],
    );
    const open = flattenTree(roots);
    assert.deepEqual(
        open.map((r) => [r.node.folder.id, r.depth]),
        [
            ["a", 0],
            ["b", 1],
            ["c", 1],
        ],
    );
});

test("breadcrumb runs root-first and stops on gaps and cycles", () => {
    const byId = new Map(FOLDERS.map((f) => [f.id, f]));
    assert.deepEqual(breadcrumb(byId, "b"), ["Personal", "Health"]);
    assert.deepEqual(breadcrumb(byId, "a"), ["Personal"]);
    assert.deepEqual(breadcrumb(byId, "d"), ["Orphan"]);
    assert.deepEqual(breadcrumb(byId, "nope"), []);
    const cyclic = new Map([
        ["x", { parent_id: "y", name: "X" }],
        ["y", { parent_id: "x", name: "Y" }],
    ]);
    assert.deepEqual(breadcrumb(cyclic, "x"), ["Y", "X"]);
});

test("move exclusion covers the folder and its whole subtree", () => {
    const excl = descendantIds(FOLDERS, "a");
    assert.ok(excl.has("a") && excl.has("b") && excl.has("c"));
    assert.ok(!excl.has("d"));
    assert.deepEqual([...descendantIds(FOLDERS, "b")], ["b"]);
});

test("a parent swap cycle surfaces both folders at the root", () => {
    const folders = [
        { id: "a", parent_id: "b", name: "A" },
        { id: "b", parent_id: "a", name: "B" },
        { id: "c", parent_id: "a", name: "Child" },
    ];
    const { roots, recovered } = buildTree(folders);
    // The cycle edge breaks deterministically in server order; the
    // stored relationships (including the innocent child) stay put.
    assert.deepEqual(
        roots.map((n) => n.folder.id),
        ["a", "b"],
    );
    assert.deepEqual(recovered, []);
    assert.deepEqual(
        roots[0].children.map((n) => n.folder.id),
        ["c"],
    );
    assert.equal(flattenTree(roots).length, 3);
});

test("a self-parent and a three-cycle stay visible without looping", () => {
    const folders = [
        { id: "s", parent_id: "s", name: "Self" },
        { id: "x", parent_id: "z", name: "X" },
        { id: "y", parent_id: "x", name: "Y" },
        { id: "z", parent_id: "y", name: "Z" },
    ];
    const { roots, recovered } = buildTree(folders);
    assert.deepEqual(
        roots.map((n) => n.folder.id),
        ["s", "x", "y", "z"],
    );
    assert.deepEqual(recovered, []);
    assert.equal(flattenTree(roots).length, 4);
});
