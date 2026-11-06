<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy } from 'svelte';
    import * as api from '@/api.js';
    import TagFilter from '@/components/TagFilter/index.svelte';
    import { createRequestOwner } from '@/lib/request-owner.js';
    import { resolveErrorText } from '@/lib/errors.js';
    import { ENTRY_LIST_ERRORS } from '@/components/EntryList/errors.js';

    let { scope = null, version = 0, selected = null, onFilterChange, onChanged } = $props();
    let tags = $state([]);
    let note = $state('');
    let removing = $state(null);
    const reads = createRequestOwner();
    const mutations = createRequestOwner();

    onDestroy(() => {
        reads.dispose();
        mutations.dispose();
    });

    async function load() {
        const token = reads.begin();
        const wireScope = scope ?? null;
        try {
            const next = await api.tagList(wireScope);
            if (!reads.current(token)) return;
            tags = next;
            note = '';
        } catch (error) {
            if (reads.current(token)) note = resolveErrorText(ENTRY_LIST_ERRORS, error).text;
        }
    }

    $effect(() => {
        void version;
        void scope?.kind;
        void scope?.id;
        void scope?.includeDescendants;
        load();
    });

    async function remove(name) {
        if (removing !== null) return;
        const token = mutations.begin();
        removing = name;
        try {
            await api.tagDelete(name);
            if (!mutations.current(token)) return;
            if (selected === name) onFilterChange?.(null);
            note = '';
            onChanged?.();
            return true;
        } catch (error) {
            if (mutations.current(token)) note = resolveErrorText(ENTRY_LIST_ERRORS, error).text;
            return false;
        } finally {
            if (mutations.current(token)) removing = null;
        }
    }
</script>

<div class="tags-container">
    <TagFilter
        {tags}
        {selected}
        {note}
        {removing}
        onSelect={(name) => onFilterChange?.(selected === name ? null : name)}
        onRemove={remove} />
</div>

<style src="./styles.css"></style>
