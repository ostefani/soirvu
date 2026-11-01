<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { untrack } from 'svelte';
    import * as api from '@/api.js';
    import { breadcrumb } from '@/lib/folders.js';

    let { initialQuery = '', folders = [], onPick = null, onCancel = null } = $props();

    let query = $state(initialQuery);
    let candidates = $state([]);
    let nextCursor = $state(null);
    let loading = $state(false);
    let active = $state(0);
    let error = $state('');
    let box = $state(null);

    const byId = $derived(new Map(folders.map((f) => [f.id, f])));

    function crumb(folderId) {
        if (folderId == null) return '';
        return breadcrumb(byId, folderId).join(' / ');
    }

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
            if (reset) active = 0;
        } catch {
            if (mine !== runId) return;
            error = 'Could not search entries.';
        } finally {
            if (mine === runId) loading = false;
        }
    }

    function onQueryInput() {
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

    function pick(entry) {
        onPick?.(entry);
    }

    function onKeydown(e) {
        if (e.key === 'Escape') {
            e.stopPropagation();
            onCancel?.();
            return;
        }
        if (e.key === 'ArrowDown') {
            e.preventDefault();
            active = Math.min(active + 1, candidates.length - 1);
            return;
        }
        if (e.key === 'ArrowUp') {
            e.preventDefault();
            active = Math.max(active - 1, 0);
            return;
        }
        if (e.key === 'Enter' && candidates[active]) {
            e.preventDefault();
            pick(candidates[active]);
            return;
        }
        // Focus stays inside the picker until it closes.
        if (e.key === 'Tab' && box) {
            const focusables = [...box.querySelectorAll('input, button')].filter((el) => !el.disabled);
            if (!focusables.length) return;
            const first = focusables[0];
            const last = focusables[focusables.length - 1];
            if (e.shiftKey && document.activeElement === first) {
                e.preventDefault();
                last.focus();
            } else if (!e.shiftKey && document.activeElement === last) {
                e.preventDefault();
                first.focus();
            }
        }
    }
</script>

<div class="link-picker" role="dialog" aria-modal="true" aria-label="Link to entry" bind:this={box}>
    <input
        type="search"
        placeholder="Search entries…"
        aria-label="Search entries to link"
        autofocus
        bind:value={query}
        oninput={onQueryInput}
        onkeydown={onKeydown} />
    {#if error}
        <p class="note" role="status">{error}</p>
    {:else}
        <ul class="candidates" onkeydown={onKeydown}>
            {#each candidates as c, i (c.id)}
                <li>
                    <button
                        class:active={i === active}
                        onclick={() => pick(c)}
                        onmouseenter={() => (active = i)}>
                        <span class="cand-title">{c.title ?? 'Untitled'}</span>
                        <span class="cand-meta">
                            {c.entry_date}
                            {#if c.folder_id} · {crumb(c.folder_id)}{/if}
                        </span>
                    </button>
                </li>
            {/each}
        </ul>
        {#if !loading && candidates.length === 0}
            <p class="note">No entries match.</p>
        {/if}
        {#if nextCursor}
            <button class="more" onclick={() => search(false)} disabled={loading}>
                {loading ? 'Searching…' : 'More…'}
            </button>
        {/if}
    {/if}
</div>

<style src="./styles.css"></style>
