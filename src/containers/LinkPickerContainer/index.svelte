<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { untrack } from 'svelte';
    import * as api from '@/api.js';
    import LinkPicker from '@/components/LinkPicker/index.svelte';

    let { initialQuery = '', folders = [], onPick = null, onCancel = null } = $props();

    let query = $state(initialQuery);
    let candidates = $state([]);
    let nextCursor = $state(null);
    let loading = $state(false);
    let error = $state('');

    let timer = null;
    let runId = 0;
    let queryEpoch = $state(0);

    async function search(reset) {
        // Read outside the effect's tracking: searches run on the
        // debounced epoch, not on every keystroke.
        const q = untrack(() => query);
        const cursor = untrack(() => (reset ? null : nextCursor));
        const mine = ++runId;
        loading = true;
        error = '';
        try {
            const page = await api.entryLinkCandidates(q, {
                cursor,
                limit: 20,
            });
            if (mine !== runId) return;
            candidates = reset ? page.candidates : [...candidates, ...page.candidates];
            nextCursor = page.next_cursor;
        } catch {
            if (mine !== runId) return;
            error = 'Could not search entries.';
        } finally {
            if (mine === runId) loading = false;
        }
    }

    function onQueryChange(value) {
        query = value;
        clearTimeout(timer);
        timer = setTimeout(() => queryEpoch++, 200);
    }

    // Empty query lists recent entries; every committed keystroke
    // re-runs the vault-wide two-tier search from the first page.
    $effect(() => {
        void queryEpoch;
        search(true);
        return () => {
            runId++;
            clearTimeout(timer);
        };
    });
</script>

<LinkPicker
    {query}
    {candidates}
    {nextCursor}
    {loading}
    {error}
    {folders}
    {onQueryChange}
    onMore={() => search(false)}
    {onPick}
    {onCancel} />
