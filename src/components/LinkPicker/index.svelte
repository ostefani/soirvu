<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { breadcrumb } from '@/lib/folders.js';

    let {
        query = '',
        candidates = [],
        nextCursor = null,
        loading = false,
        error = '',
        folders = [],
        onQueryChange = null,
        onMore = null,
        onPick = null,
        onCancel = null,
    } = $props();

    let active = $state(0);
    let box = $state(null);

    const byId = $derived(new Map(folders.map((f) => [f.id, f])));

    function crumb(folderId) {
        if (folderId == null) return '';
        return breadcrumb(byId, folderId).join(' / ');
    }

    // A new result set restarts the highlight; paging appends, so
    // the highlight stays where the keyboard left it. The query is
    // container-owned: it changes only on a fresh search.
    $effect(() => {
        void query;
        active = 0;
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
        // Native activation already picks the focused button (a
        // candidate or More…): handling Enter here too would fire
        // both the focused row and the highlighted one.
        if (e.target instanceof HTMLButtonElement) return;
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

<!-- One dialog-boundary handler: input, results and the More…
     footer all funnel through it, so Tab and Escape cannot bypass
     the trap from the footer. -->
<div
    class="link-picker"
    role="dialog"
    aria-modal="true"
    aria-label="Link to entry"
    bind:this={box}
    onkeydown={onKeydown}>
    <input
        type="search"
        placeholder="Search entries…"
        aria-label="Search entries to link"
        autofocus
        value={query}
        oninput={(e) => onQueryChange?.(e.currentTarget.value)} />
    {#if error}
        <p class="note" role="status">{error}</p>
    {:else}
        <ul class="candidates">
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
            <button class="more" onclick={onMore} disabled={loading}>
                {loading ? 'Searching…' : 'More…'}
            </button>
        {/if}
    {/if}
</div>

<style src="./styles.css"></style>
