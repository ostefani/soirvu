// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import * as api from '@/api.js';
import { createRequestOwner } from '@/lib/request-owner.js';
import { searchScopeKey } from '@/lib/scope.js';

const all = () => ({ kind: 'all' });

/**
 * The folder tree and the scope selected in it. The scope drives the
 * entry list, calendar, tags and (optionally) search. Call from
 * component initialisation: the request owner is disposed with the
 * component.
 *
 * Invariants are enforced where state changes, not by effects that
 * repair it afterwards:
 *  - the selection never points at a folder missing from a loaded tree;
 *  - "search only in this folder" is on only while a folder is selected.
 *
 * Vault I/O arrives through `deps` (the `@/api.js` namespace unless a
 * caller passes fakes), so asynchronous behavior can be exercised
 * without native Tauri.
 */
export function createFolderScope(deps = api) {
    const owner = createRequestOwner();

    // API payloads and selections are replaced wholesale, never mutated.
    let folders = $state.raw([]);
    let selection = $state.raw(all());
    let searchScoped = $state(false);
    let loaded = false;

    const byId = $derived(new Map(folders.map((f) => [f.id, f])));

    // What the read models receive. Null reads the whole vault; the
    // server defaults a missing scope to All, so only folder/root travel.
    const listScope = $derived.by(() => {
        if (selection.kind === 'folder') {
            return { kind: 'folder', id: selection.id, includeDescendants: !!selection.includeDescendants };
        }
        return selection.kind === 'root' ? { kind: 'root' } : null;
    });

    const folderName = $derived(selection.kind === 'folder' ? (byId.get(selection.id)?.name ?? '…') : null);
    const searchScope = $derived(searchScoped ? listScope : null);
    const searchKey = $derived(searchScopeKey(searchScoped, listScope));

    // A folder deleted elsewhere drops back to the whole vault.
    // Before the first load every folder is trusted.
    function live(sel) {
        return loaded && sel.kind === 'folder' && !byId.has(sel.id) ? all() : sel;
    }

    function setFolders(next) {
        folders = next;
        loaded = true;
        const current = live(selection);
        if (current !== selection) {
            selection = current;
            searchScoped = false;
        }
    }

    async function load() {
        const token = owner.begin();
        try {
            const next = await deps.folderList();
            if (owner.current(token)) setFolders(next);
        } catch {
            // Keep the last good tree: counts may lag after a failed
            // refresh, but the navigator must not vanish under the list.
        }
    }

    /** User picked a scope. Picking a folder keeps the search checkbox. */
    function select(sel) {
        selection = live(sel);
        if (selection.kind !== 'folder') searchScoped = false;
    }

    /** History restore: scope and checkbox together. */
    function restore(sel, scoped) {
        selection = live(sel);
        searchScoped = !!scoped && selection.kind === 'folder';
    }

    function setSearchScoped(value) {
        searchScoped = !!value && selection.kind === 'folder';
    }

    function toggleDescendants() {
        if (selection.kind !== 'folder') return;
        selection = { ...selection, includeDescendants: !selection.includeDescendants };
    }

    // Disposed owners publish nothing.
    $effect(() => () => owner.dispose());

    return {
        get folders() {
            return folders;
        },
        get selection() {
            return selection;
        },
        get listScope() {
            return listScope;
        },
        get folderName() {
            return folderName;
        },
        /** Real folder new entries file into; null in the All and Root views. */
        get folderId() {
            return selection.kind === 'folder' ? selection.id : null;
        },
        get searchScoped() {
            return searchScoped;
        },
        /** Scope sent with a search; null searches the whole vault. */
        get searchScope() {
            return searchScope;
        },
        /** Changes whenever the inputs a visible search depends on change. */
        get searchKey() {
            return searchKey;
        },
        load,
        select,
        restore,
        setSearchScoped,
        toggleDescendants,
    };
}
