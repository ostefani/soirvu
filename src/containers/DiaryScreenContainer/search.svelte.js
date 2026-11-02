// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import { untrack } from 'svelte';
import * as api from '@/api.js';
import { createRequestOwner } from '@/lib/request-owner.js';
import { shouldReissueSearch } from '@/lib/scope.js';

const DEBOUNCE_MS = 250;

function failureNote(e) {
    return e === 'query_too_short' ? 'Type at least 3 characters (2 for Russian).' : `Search failed (${e}).`;
}

/**
 * Debounced search over the whole vault or the selected folder.
 * `scope` is a folder scope exposing `searchScope` (sent on the wire,
 * null = whole vault) and `searchKey` (changes when either input
 * changes). Call from component initialisation: the timer and request
 * owner are torn down with the component.
 */
export function createSearch(scope) {
    const owner = createRequestOwner();

    let query = $state('');
    let results = $state.raw(null); // null = not searching
    let note = $state('');
    let timer;
    // The scope key the last search ran under. A visible search re-runs
    // when its scope moves; recording the key here keeps a search's own
    // completion from re-firing it.
    let lastKey = '';

    async function run(q) {
        const token = owner.begin();
        lastKey = scope.searchKey;
        const wireScope = scope.searchScope;
        try {
            const found = await api.search(q, wireScope ? { scope: wireScope } : {});
            if (!owner.current(token)) return;
            results = found;
            note = found.length === 0 ? 'Nothing found.' : '';
        } catch (e) {
            if (!owner.current(token)) return;
            results = null;
            note = failureNote(e);
        }
    }

    function dismiss() {
        results = null;
        note = '';
    }

    async function flush() {
        const q = query.trim();
        if (q) await run(q);
        else dismiss();
    }

    /** Sets the query and schedules a debounced search. */
    function setQuery(value) {
        query = value;
        clearTimeout(timer);
        // New intent retires the in-flight search immediately, before
        // this keystroke's debounce expires.
        owner.invalidate();
        timer = setTimeout(flush, DEBOUNCE_MS);
    }

    /** Empties the query and the results; drops any pending search. */
    function clear() {
        clearTimeout(timer);
        owner.invalidate();
        query = '';
        dismiss();
    }

    /** Puts back a remembered query (history) and searches it at once. */
    async function restore(value) {
        query = value;
        clearTimeout(timer);
        const q = value.trim();
        if (q) {
            await run(q);
        } else {
            owner.invalidate();
            dismiss();
        }
    }

    /** Re-runs the visible search against fresh data. */
    function refresh() {
        const q = query.trim();
        if (q) run(q);
    }

    $effect(() => {
        const key = scope.searchKey;
        untrack(() => {
            const q = query.trim();
            if (shouldReissueSearch(key, lastKey, q)) run(q);
        });
    });

    $effect(() => () => {
        clearTimeout(timer);
        owner.dispose();
    });

    return {
        get query() {
            return query;
        },
        get results() {
            return results;
        },
        get note() {
            return note;
        },
        setQuery,
        clear,
        restore,
        refresh,
        /** Retires the in-flight search without touching what is shown. */
        invalidate: () => owner.invalidate(),
    };
}
