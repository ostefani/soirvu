<script>
    // The entry column: newest first, keyset pages, tag filter,
    // and full-text search. Search snippets arrive with
    // \u0002/\u0003 highlight markers and are rendered as text
    // segments with <mark> — diary text never touches {@html}.
    import * as api from "../api.js";

    let { selectedId, onSelect, date = null, onClearDate } = $props();

    let entries = $state([]);
    let nextCursor = $state(null);
    let tags = $state([]);
    let tagFilter = $state(null);
    let query = $state("");
    let results = $state(null); // null = not searching
    let searchNote = $state("");
    let searchTimer = null;
    let error = $state("");

    // Same short texts as the other screens: codes, never prose
    // from below.
    function describeError(e) {
        switch (String(e)) {
            case "locked":
                return "The vault is locked.";
            case "invalid_input":
                return "That was not accepted (invalid_input).";
            case "storage":
                return "A storage problem (storage).";
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
            nextCursor = page.next_cursor;
            error = "";
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

    function today() {
        const d = new Date();
        const pad = (n) => String(n).padStart(2, "0");
        return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    }

    async function newEntry() {
        try {
            const created = await api.entrySave({
                id: null,
                entry_date: today(),
                title: null,
                body_md: "",
                mood: null,
                tz: Intl.DateTimeFormat().resolvedOptions().timeZone,
                tags: [],
            });
            await refresh();
            onSelect(created.id);
        } catch (e) {
            error = describeError(e);
        }
    }

    function segments(snippet) {
        const out = [];
        let marked = false;
        let buf = "";
        for (const ch of snippet) {
            if (ch === "\u0002" || ch === "\u0003") {
                if (buf) out.push({ text: buf, marked });
                buf = "";
                marked = ch === "\u0002";
            } else {
                buf += ch;
            }
        }
        if (buf) out.push({ text: buf, marked });
        return out;
    }

    function onSearchInput() {
        clearTimeout(searchTimer);
        searchTimer = setTimeout(async () => {
            const q = query.trim();
            if (!q) {
                results = null;
                searchNote = "";
                return;
            }
            try {
                results = await api.search(q);
                searchNote = results.length === 0 ? "Nothing found." : "";
            } catch (e) {
                results = null;
                searchNote =
                    e === "query_too_short"
                        ? "Type at least 3 characters (2 for Russian)."
                        : `Search failed (${e}).`;
            }
        }, 250);
    }

    function pickTag(name) {
        tagFilter = tagFilter === name ? null : name;
        results = null;
        query = "";
        load(true);
    }

    // The section folds away for people who do not use tags.
    // The × on a tag (shown on hover) removes it from every
    // entry at once; removing a tag from one entry is done in
    // that entry's own tag field. Two-step, like entry deletion.
    let tagsOpen = $state(true);
    let confirmRemove = $state(null); // tag name awaiting confirmation
    let tagNote = $state("");

    async function removeTag(name) {
        try {
            await api.tagDelete(name);
            if (tagFilter === name) tagFilter = null;
            confirmRemove = null;
            tagNote = "";
            await refresh();
        } catch (e) {
            tagNote = describeError(e);
        }
    }
</script>

<div class="entry-list">
    <div class="list-top">
        <button onclick={newEntry}>New entry</button>
        <input
            type="search"
            placeholder="Search the diary…"
            bind:value={query}
            oninput={onSearchInput}
        />
    </div>

    {#if error}<p class="form-error">{error}</p>{/if}

    {#if date}
        <p class="date-filter">
            Entries for {date}
            <button class="quiet" onclick={() => onClearDate?.()}>Show all</button>
        </p>
    {/if}

    {#if tags.length}
        <div class="tags-block">
            <button class="quiet tags-toggle" onclick={() => (tagsOpen = !tagsOpen)}>
                {tagsOpen ? "▾" : "▸"} Tags
            </button>
            {#if tagsOpen}
                <div class="tag-flow">
                    {#each tags as t (t.name)}
                        <span class="tag-wrap">
                            {#if confirmRemove === t.name}
                                <span class="confirm-row">
                                    <span>Remove “{t.name}” from all {t.entries} entries?</span>
                                    <button class="quiet mini" onclick={() => removeTag(t.name)}>Remove</button>
                                    <button class="quiet mini" onclick={() => (confirmRemove = null)}>Cancel</button>
                                </span>
                            {:else}
                                <button
                                    class="chip"
                                    class:active={tagFilter === t.name}
                                    onclick={() => pickTag(t.name)}
                                >
                                    {t.name}
                                </button>
                                <span class="tag-count">{t.entries}</span>
                                <button
                                    class="tag-x"
                                    title="Remove this tag from all entries"
                                    onclick={() => (confirmRemove = t.name)}
                                >
                                    ×
                                </button>
                            {/if}
                        </span>
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
                    <span class="result-head">{r.entry_date} — {r.title ?? "Untitled"}</span>
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
                <button
                    class="entry-row"
                    class:active={selectedId === entry.id}
                    onclick={() => onSelect(entry.id)}
                >
                    <span class="entry-date">{entry.entry_date}</span>
                    <span class="entry-title">{entry.title ?? entry.excerpt ?? "Untitled"}</span>
                    
                </button>
            {/each}
            {#if entries.length === 0}<p class="note">No entries yet.</p>{/if}
            {#if nextCursor}
                <button class="quiet" onclick={() => load(false)}>More…</button>
            {/if}
        </div>
    {/if}
</div>

<style>
    /* The entry list, tag chips and search results. */
    .entry-list {
        padding: 0.5rem;
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .list-top {
        display: flex;
        gap: 0.4rem;
    }

    .list-top input {
        flex: 1;
        min-width: 0;
    }

    .tags-toggle {
        align-self: flex-start;
        font-size: 0.9em;
    }

    .tag-flow {
        display: flex;
        flex-wrap: wrap;
        gap: 0.35rem 0.6rem;
        margin-top: 0.3rem;
    }

    .tag-wrap {
        display: inline-flex;
        align-items: center;
        gap: 0.2rem;
    }

    .tag-count {
        font-size: 0.8em;
        opacity: 0.65;
    }

    button.mini {
        font-size: 0.8em;
        padding: 0.1em 0.4em;
    }

    .tag-x {
        opacity: 0;
        border: none;
        background: none;
        padding: 0 0.25em;
        font-size: 1em;
        line-height: 1;
        cursor: pointer;
    }

    .tag-wrap:hover .tag-x,
    .tag-x:focus-visible {
        opacity: 0.7;
    }

    .confirm-row {
        display: flex;
        align-items: center;
        gap: 0.3rem;
        flex-wrap: wrap;
        font-size: 0.9em;
    }
    button.chip {
        padding: 0.15em 0.7em;
        font-size: 0.85em;
        border-radius: 999px;
    }

    .entries,
    .results {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    button.entry-row,
    button.result {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 0.1rem;
        text-align: left;
        width: 100%;
    }

    .entry-date {
        font-size: 0.8em;
        opacity: 0.65;
    }

    .entry-title {
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 100%;
    }

    .result-head {
        font-weight: 600;
        font-size: 0.9em;
    }

    .snippet {
        font-size: 0.9em;
        opacity: 0.85;
    }

    .snippet mark {
        background: #ffe58a;
        color: inherit;
    }
</style>
