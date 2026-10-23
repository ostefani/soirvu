<script>
    import * as api from '@/api.js';

    let {
        selectedId,
        onSelect,
        date = null,
        onClearDate,
        // Search state lives in the screen; the list only renders it.
        results = null, // null = not searching
        searchNote = '',
        onClearSearch,
    } = $props();

    let entries = $state([]);
    let nextCursor = $state(null);
    let tags = $state([]);
    let tagFilter = $state(null);
    let error = $state('');

    // Same short texts as the other screens: codes, never prose
    // from below.
    function describeError(e) {
        switch (String(e)) {
            case 'locked':
                return 'The vault is locked.';
            case 'invalid_input':
                return 'That was not accepted (invalid_input).';
            case 'storage':
                return 'A storage problem (storage).';
            default:
                return `Something went wrong (${String(e)}).`;
        }
    }

    async function load(reset) {
        try {
            const page = await api.entryList({
                cursor: reset ? null : nextCursor,
                limit: 50,
                tag: tagFilter,
                date,
            });
            entries = reset ? page.entries : [...entries, ...page.entries];
            console.log('entries: ', entries);
            nextCursor = page.next_cursor;
            error = '';
        } catch (e) {
            error = describeError(e);
        }
    }

    async function loadTags() {
        tags = await api.tagList();
    }

    export async function refresh() {
        await Promise.all([load(true), loadTags()]);
    }

    $effect(() => {
        refresh();
    });

    function segments(snippet) {
        const out = [];
        let marked = false;
        let buf = '';
        for (const ch of snippet) {
            if (ch === '\u0002' || ch === '\u0003') {
                if (buf) out.push({ text: buf, marked });
                buf = '';
                marked = ch === '\u0002';
            } else {
                buf += ch;
            }
        }
        if (buf) out.push({ text: buf, marked });
        return out;
    }

    function pickTag(name) {
        tagFilter = tagFilter === name ? null : name;
        // A fresh filter needs the plain list, not stale search
        // results — the screen owns that state and resets it.
        onClearSearch?.();
        load(true);
    }

    // The section folds away for people who do not use tags.
    // The × on a tag (shown on hover) removes it from every
    // entry at once; removing a tag from one entry is done in
    // that entry's own tag field. Two-step, like entry deletion.
    let tagsOpen = $state(false);
    let confirmRemove = $state(null); // tag name awaiting confirmation
    let tagNote = $state('');

    async function removeTag(name) {
        try {
            await api.tagDelete(name);
            if (tagFilter === name) tagFilter = null;
            confirmRemove = null;
            tagNote = '';
            await refresh();
        } catch (e) {
            tagNote = describeError(e);
        }
    }
</script>

<div class="entry-list">
    {#if error}<p class="form-error">{error}</p>{/if}

    {#if date}
        <p class="date-filter">
            Entries for {date}
            <button class="quiet" onclick={() => onClearDate?.()}>Show all</button>
        </p>
    {/if}

    {#if tags.length}
        <div class="tags-block">
            <button class="tags-toggle" onclick={() => (tagsOpen = !tagsOpen)} aria-expanded={tagsOpen}>
                <span class="tags-toggle-chevron">{tagsOpen ? '▾' : '▸'}</span>
                <span class="tags-toggle-label">Tags</span>
                <span class="tags-toggle-count">{tags.length}</span>
            </button>
            {#if tagsOpen}
                <div class="tag-flow">
                    {#each tags as t (t.name)}
                        {#if confirmRemove === t.name}
                            <div class="tag-confirm">
                                <span class="confirm-message">Remove “{t.name}” from all {t.entries} entries?</span>
                                <div class="confirm-actions">
                                    <button class="confirm-action remove" onclick={() => removeTag(t.name)}
                                        >Remove</button>
                                    <button class="confirm-action cancel" onclick={() => (confirmRemove = null)}
                                        >Cancel</button>
                                </div>
                            </div>
                        {:else}
                            <div class="tag-pill" class:active={tagFilter === t.name}>
                                <button
                                    class="tag-button"
                                    class:active={tagFilter === t.name}
                                    title={tagFilter === t.name ? `Clear filter: ${t.name}` : `Filter by: ${t.name}`}
                                    onclick={() => pickTag(t.name)}>
                                    <span class="tag-name">{t.name}</span>
                                    <span class="tag-count">{t.entries}</span>
                                </button>
                                <button
                                    class="tag-remove"
                                    title="Remove this tag from all entries"
                                    aria-label="Remove tag {t.name}"
                                    onclick={() => (confirmRemove = t.name)}>
                                    ×
                                </button>
                            </div>
                        {/if}
                    {/each}
                </div>
                {#if tagNote}<p class="note">{tagNote}</p>{/if}
            {/if}
        </div>
    {/if}

    {#if searchNote}<p class="note">{searchNote}</p>{/if}

    {#if results !== null}
        <div class="results">
            {#each results as r (r.id)}
                <button class="result" onclick={() => onSelect(r.id)}>
                    <span class="result-head">{r.entry_date} — {r.title ?? 'Untitled'}</span>
                    <span class="snippet">
                        {#each segments(r.snippet) as seg}
                            {#if seg.marked}<mark>{seg.text}</mark>{:else}{seg.text}{/if}
                        {/each}
                    </span>
                </button>
            {/each}
        </div>
    {:else}
        <div class="entries">
            {#each entries as entry (entry.id)}
                <button class="entry-row" class:active={selectedId === entry.id} onclick={() => onSelect(entry.id)}>
                    <span class="entry-date">{entry.entry_date}</span>
                    <span class="entry-title">{entry.title ?? entry.excerpt ?? 'Untitled'}</span>
                </button>
            {/each}
            {#if entries.length === 0}<p class="note">No entries yet.</p>{/if}
            {#if nextCursor}
                <button class="quiet" onclick={() => load(false)}>More…</button>
            {/if}
        </div>
    {/if}
</div>

<style src="./styles.css"></style>
