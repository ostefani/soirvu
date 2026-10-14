<script>
    // One entry: metadata fields over the ProseMirror editor.
    // Autosave: any change (typing, title, tags, mood, date) marks
    // the entry dirty and saves after a quiet 900 ms. The editor's
    // onChange only fires on real edits, so merely opening an entry
    // never saves it — the baseline rule (§6.5).
    import { onDestroy, onMount } from "svelte";
    import * as api from "../api.js";
    import { createEditor } from "../editor/index.js";

    let { entryId, onDeleted, onSaved } = $props();

    let entry = $state(null);
    let title = $state("");
    let entryDate = $state("");
    let mood = $state("");
    let tagsText = $state("");
    let status = $state("");
    let deleteArmed = $state(false);
    let mountEl = $state(null);

    let editor = null;
    let saveTimer = null;
    let dirty = false;

    function markDirty() {
        dirty = true;
        status = "Not saved yet";
        clearTimeout(saveTimer);
        saveTimer = setTimeout(save, 900);
    }

    async function save() {
        if (!dirty || !entry || !editor) return;
        status = "Saving…";
        try {
            const saved = await api.entrySave({
                id: entry.id,
                entry_date: entryDate,
                title: title.trim() === "" ? null : title.trim(),
                body_md: editor.getMarkdown(),
                mood: mood === "" ? null : Number(mood),
                tz: entry.tz,
                tags: tagsText
                    .split(",")
                    .map((t) => t.trim())
                    .filter((t) => t !== ""),
            });
            entry = saved;
            dirty = false;
            status = "Saved";
            onSaved?.();
        } catch (e) {
            status = `Could not save (${e})`;
        }
    }

    onMount(async () => {
        entry = await api.entryGet(entryId);
        title = entry.title ?? "";
        entryDate = entry.entry_date;
        mood = entry.mood == null ? "" : String(entry.mood);
        tagsText = entry.tags.join(", ");
    });

    // The editor is created by an effect, not in onMount: the mount
    // element only exists after the {#if entry} block has rendered,
    // which happens after onMount's await resolves — mountEl is
    // still null there. The effect runs once both are set. It must
    // NOT return a cleanup that destroys the editor: `entry` is
    // reassigned by every save, which re-runs the effect; the guard
    // makes those runs no-ops, and onDestroy does the teardown.
    $effect(() => {
        if (!entry || !mountEl || editor) return;
        editor = createEditor(mountEl, {
            markdown: entry.body_md,
            onChange: () => markDirty(),
            onOpenLink: (href) => {
                if (href.startsWith("https://") || href.startsWith("mailto:")) {
                    api.openLink(href).catch(() => {});
                }
            },
        });
    });

    onDestroy(() => {
        clearTimeout(saveTimer);
        // One last save attempt for the tail of an edit. If the
        // vault has just locked, this fails with "locked" and the
        // tail is lost — autosave's 900 ms window keeps that tail
        // under a sentence in practice.
        if (dirty) save();
        editor?.destroy();
    });

    async function remove() {
        await api.entryDelete(entry.id);
        onDeleted();
    }
</script>

{#if entry}
    <div class="editor-pane">
        <div class="meta-row">
            <input class="title-input" placeholder="Title" bind:value={title} oninput={markDirty} />
            <input type="date" bind:value={entryDate} onchange={markDirty} />
            <select bind:value={mood} onchange={markDirty} aria-label="Mood">
                <option value="">Mood</option>
                {#each [1, 2, 3, 4, 5] as m}<option value={String(m)}>{m}</option>{/each}
            </select>
        </div>
        <input
            class="tags-input"
            placeholder="Tags, comma separated"
            bind:value={tagsText}
            oninput={markDirty}
        />
        <div class="editor-mount" bind:this={mountEl}></div>
        <div class="editor-foot">
            <span class="status">{status}</span>
            {#if deleteArmed}
                <button onclick={remove}>Really delete this entry?</button>
                <button class="quiet" onclick={() => (deleteArmed = false)}>Keep it</button>
            {:else}
                <button class="quiet" onclick={() => (deleteArmed = true)}>Delete entry</button>
            {/if}
        </div>
    </div>
{:else}
    <p class="note">Opening…</p>
{/if}
