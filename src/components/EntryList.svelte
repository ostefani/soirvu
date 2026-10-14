<script>
    // The entry column: newest first, keyset pages, tag filter,
    // and full-text search. Search snippets arrive with
    // \u0002/\u0003 highlight markers and are rendered as text
    // segments with <mark> — diary text never touches {@html}.
    import * as api from "../api.js";

    let { selectedId, onSelect } = $props();

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
            });
            entries = reset ? page.entries : [...entries, ...page.entries];
            nextCursor = page.next_cursor;
            error = "";
        } catch (e) {
            error = describeError(e);
        }
    }

    async function loadTags() {
        tags = (await api.tagList()).map((t) => t.name);
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

    {#if tags.length}
        <div class="tag-row">
            {#each tags as name}
                <button class="chip" class:active={tagFilter === name} onclick={() => pickTag(name)}>
                    {name}
                </button>
            {/each}
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
