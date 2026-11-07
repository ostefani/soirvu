// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Pure folder-tree helpers: no IPC, no Svelte. The server lists
// folders flat and pre-sorted (normalized name, id tie-break);
// this module only nests, flattens and names. Tested with node:test.

/**
 * Nest a flat server-ordered folder list into a forest. Children
 * keep their relative order, so every level stays in server
 * sort. Folders whose stored parent is gone (deleted elsewhere,
 * sync-ordered data) cannot hang under it: they collect in
 * `recovered` for display at the root. Offline moves can also
 * arrive as a parent cycle (A under B under A): no member of a
 * cycle hangs under its stored parent, or the whole component
 * would vanish from the tree. Cycle members display at the root
 * in server order with their subtrees; the stored relationships
 * are untouched, so the next sync can still heal them.
 * @param {{ id: string, parent_id: string | null, name: string }[]} folders
 * @returns {{ roots: Node[], recovered: Node[] }}
 */
export function buildTree(folders) {
    const byId = new Map(folders.map((f) => [f.id, { folder: f, children: [] }]));
    const cyclic = findCycleIds(folders);
    const roots = [];
    const recovered = [];
    for (const f of folders) {
        const node = byId.get(f.id);
        if (f.parent_id == null || cyclic.has(f.id)) {
            roots.push(node);
        } else if (byId.has(f.parent_id)) {
            byId.get(f.parent_id).children.push(node);
        } else {
            recovered.push(node);
        }
    }
    return { roots, recovered };
}

/**
 * Ids on a directed parent-pointer cycle: offline moves accepted
 * by sync can point A at B and B back at A, and a folder can even
 * point at itself. Iterative two-color walk over stored parents
 * (unknown parents end the chain); deterministic in server order.
 * @param {{ id: string, parent_id: string | null }[]} folders
 * @returns {Set<string>}
 */
function findCycleIds(folders) {
    const parentOf = new Map(folders.map((f) => [f.id, f.parent_id]));
    const done = new Set();
    const cyclic = new Set();
    for (const f of folders) {
        let cur = f.id;
        const chain = [];
        const onChain = new Set();
        while (cur != null && parentOf.has(cur) && !done.has(cur)) {
            if (onChain.has(cur)) {
                for (let i = chain.indexOf(cur); i < chain.length; i++) {
                    cyclic.add(chain[i]);
                }
                break;
            }
            onChain.add(cur);
            chain.push(cur);
            cur = parentOf.get(cur);
        }
        for (const id of chain) done.add(id);
    }
    return cyclic;
}

/**
 * Pre-order walk with depth, skipping collapsed subtrees.
 * @param {Node[]} nodes
 * @param {Set<string>} [collapsed]
 */
export function flattenTree(nodes, collapsed = new Set()) {
    const out = [];
    const walk = (node, depth) => {
        out.push({ node, depth });
        if (!collapsed.has(node.folder.id)) {
            for (const child of node.children) walk(child, depth + 1);
        }
    };
    for (const node of nodes) walk(node, 0);
    return out;
}

/**
 * Root-first names from a folder up to its top ancestor. Stops
 * at unknown ids and on cycles instead of looping.
 * @param {Map<string, { parent_id: string | null, name: string }>} byId
 * @param {string | null} folderId
 * @returns {string[]}
 */
export function breadcrumb(byId, folderId) {
    const names = [];
    const seen = new Set();
    let cur = folderId;
    while (cur != null && !seen.has(cur)) {
        seen.add(cur);
        const f = byId.get(cur);
        if (!f) break;
        names.unshift(f.name);
        cur = f.parent_id;
    }
    return names;
}

/**
 * A folder plus every id below it: the exclusion set for move
 * destinations (a folder cannot go inside itself).
 * @param {{ id: string, parent_id: string | null }[]} folders
 * @param {string} id
 * @returns {Set<string>}
 */
export function descendantIds(folders, id) {
    const kids = new Map();
    for (const f of folders) {
        if (f.parent_id != null) {
            if (!kids.has(f.parent_id)) kids.set(f.parent_id, []);
            kids.get(f.parent_id).push(f.id);
        }
    }
    const out = new Set([id]);
    const stack = [id];
    while (stack.length > 0) {
        for (const c of kids.get(stack.pop()) ?? []) {
            if (!out.has(c)) {
                out.add(c);
                stack.push(c);
            }
        }
    }
    return out;
}
