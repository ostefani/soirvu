<script>
    // One entry: metadata fields over the ProseMirror editor.
    // Autosave: any change (typing, title, tags, mood, date) marks
    // the entry dirty and saves after a quiet 900 ms. The editor's
    // onChange only fires on real edits, so merely opening an entry
    // never saves it — the baseline rule (§6.5).
    import { onDestroy, onMount } from 'svelte';
    import * as api from '../api.js';
    import { createEditor } from '../editor/index.js';
    import EditorToolbar from './EditorToolbar.svelte';

    let { entryId, onDeleted, onSaved, onBack } = $props();

    let entry = $state(null);
    let title = $state('');
    let entryDate = $state('');
    let mood = $state('');
    let tagsText = $state('');
    let status = $state('');
    let deleteArmed = $state(false);
    let mountEl = $state(null);
    let fileInput = $state(null);
    let info = $state(null);

    let editor = null;
    let saveTimer = null;
    let dirty = false;

    // The toolbar is a fixed bar in the window (§6.5), always
    // visible; this only keeps its active states current — the
    // block and marks at the cursor or over the selection. It
    // runs after every editor transaction.
    function refreshToolbar() {
        info = editor ? editor.query() : null;
    }

    function markDirty() {
        dirty = true;
        status = 'Not saved yet';
        clearTimeout(saveTimer);
        saveTimer = setTimeout(save, 900);
    }

    async function save() {
        if (!dirty || !entry || !editor) return;
        status = 'Saving…';
        try {
            const saved = await api.entrySave({
                id: entry.id,
                entry_date: entryDate,
                title: title.trim() === '' ? null : title.trim(),
                body_md: editor.getMarkdown(),
                mood: mood === '' ? null : Number(mood),
                tz: entry.tz,
                tags: tagsText
                    .split(',')
                    .map((t) => t.trim())
                    .filter((t) => t !== ''),
            });
            entry = saved;
            dirty = false;
            status = 'Saved';
            onSaved?.();
        } catch (e) {
            status = `Could not save (${e})`;
        }
    }

    onMount(async () => {
        entry = await api.entryGet(entryId);
        title = entry.title ?? '';
        entryDate = entry.entry_date;
        mood = entry.mood == null ? '' : String(entry.mood);
        tagsText = entry.tags.join(', ');
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
        mountEl.addEventListener('dblclick', (e) => {
            const img = e.target.closest('img');
            const src = img?.getAttribute('src') || '';
            if (src.startsWith('soirvu-asset://localhost/')) {
                api.attachmentOpen(src.slice('soirvu-asset://localhost/'.length)).catch(() => {});
            }
        });
        editor = createEditor(mountEl, {
            markdown: entry.body_md,
            onChange: () => markDirty(),
            onSelectionChange: refreshToolbar,
            onAttach: (file) => attachFile(file),
            onAttachmentView: (el, id) => hydrateAttachment(el, id),
            onOpenLink: (href) => {
                if (href.startsWith('https://') || href.startsWith('mailto:')) {
                    api.openLink(href).catch(() => {});
                }
            },
        });
    });

    // Attachments (§6.5): the file is read in 1 MiB slices and
    // sent through the chunked upload; Rust sniffs, caps and
    // strips it, and the editor inserts the node. A paste or drop
    // of files lands here too, through the editor's onAttach.
    const ATTACH_CHUNK = 1024 * 1024;

    async function attachFile(file) {
        if (!entry || !editor) return;
        status = `Attaching ${file.name}…`;
        let uploadId = null;
        try {
            uploadId = await api.attachmentBegin(entry.id, file.name, file.type || null);
            for (let offset = 0; offset < file.size; offset += ATTACH_CHUNK) {
                const slice = await file.slice(offset, offset + ATTACH_CHUNK).arrayBuffer();
                await api.attachmentChunk(uploadId, new Uint8Array(slice));
            }
            const added = await api.attachmentFinish(uploadId);
            uploadId = null;
            editor.insertAttachment({ id: added.id, mime: added.mime, name: file.name });
            status = 'Saved';
        } catch (e) {
            if (uploadId) api.attachmentAbort(uploadId).catch(() => {});
            if (e === 'too_large') status = 'That file is too large for its type.';
            else if (e === 'unsupported_type') status = 'That file type is not supported.';
            else if (e === 'locked') status = 'The diary is locked.';
            else status = `Could not attach the file (${e}).`;
        }
    }

    function onFilesPicked() {
        for (const file of fileInput.files) attachFile(file);
        fileInput.value = '';
    }

    // Fills an attachment block with its viewer (§6.4): the type
    // comes from Rust (attachment_info), never from the file name.
    async function hydrateAttachment(el, id) {
        try {
            const info = await api.attachmentInfo(id);
            el.replaceChildren();
            const url = api.assetUrl(info.id);
            if (info.mime.startsWith('video/')) {
                const v = document.createElement('video');
                v.controls = true;
                v.src = url;
                el.appendChild(v);
            } else if (info.mime.startsWith('audio/')) {
                const a = document.createElement('audio');
                a.controls = true;
                a.src = url;
                el.appendChild(a);
            } else {
                const label = document.createElement('span');
                label.textContent = `${info.name} · ${Math.max(1, Math.round(info.size / 1024))} KB`;
                el.appendChild(label);
            }
            el.title = 'Double-click to open';
            el.addEventListener('dblclick', () => {
                api.attachmentOpen(info.id).catch(() => {});
            });
        } catch {
            el.textContent = 'Attachment unavailable.';
        }
    }

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
        <div class="back-row">
            <button class="quiet" onclick={() => onBack?.()}>‹ Back</button>
        </div>
        <div class="meta-row">
            <input class="title-input" placeholder="Title" bind:value={title} oninput={markDirty} />
            <input type="date" bind:value={entryDate} onchange={markDirty} />
            <select bind:value={mood} onchange={markDirty} aria-label="Mood">
                <option value="">Mood</option>
                {#each [1, 2, 3, 4, 5] as m}<option value={String(m)}>{m}</option>{/each}
            </select>
        </div>
        <input class="tags-input" placeholder="Tags, comma separated" bind:value={tagsText} oninput={markDirty} />
        <EditorToolbar {editor} {info} onAttach={() => fileInput?.click()} />
        <input
            type="file"
            multiple
            hidden
            bind:this={fileInput}
            onchange={onFilesPicked}
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

<style>
    /* Spaces render as typed: with the default white-space a
       trailing space collapses until the next letter lands, so
       typing looks like spaces are being eaten. break-spaces
       (pre-wrap as the older-engine fallback) keeps every space
       visible, which is what a text editor owes the typist. */
    .editor-pane {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        max-width: 46rem;
    }

    .meta-row {
        display: flex;
        gap: 0.4rem;
    }

    .title-input {
        flex: 1;
        min-width: 0;
        font-weight: 600;
    }

    .tags-input {
        width: 100%;
        box-sizing: border-box;
    }

    .editor-mount {
        border: 1px solid rgba(127, 127, 127, 0.35);
        border-radius: 8px;
        padding: 0.75rem 1rem;
        min-height: 40vh;
    }

    .editor-foot {
        display: flex;
        justify-content: space-between;
        align-items: center;
    }

    .status {
        font-size: 0.85em;
        opacity: 0.7;
    }

    :global(.editor-mount.ProseMirror) {
        outline: none;
        min-height: 38vh;
        white-space: pre-wrap;
        white-space: break-spaces;
    }

    /* Editor content styles. The editor's DOM is rendered by
       editor/view.js, not by Svelte, so these rules are :global
       but fenced under .editor-mount — this component is their
       owner. Without them a task item looks like a bullet and
       inline code looks like a code block: the document keeps
       them distinct (li[data-checked], <code> vs <pre>), only
       the presentation was missing. */
    .editor-mount :global(li[data-checked]) {
        list-style: none;
        position: relative;
    }

    /* Absolute, not inline: the item's content is a block <p>,
       so an inline box would sit on its own line above the text.
       The gutter left of the item is where the box belongs —
       and where the view listens for toggle clicks. */
    .editor-mount :global(li[data-checked]::before) {
        position: absolute;
        left: -1.45em;
        top: 0.08em;
        font-size: 1.05em;
        line-height: 1;
    }

    .editor-mount :global(li[data-checked="false"]::before) {
        content: "☐";
    }

    .editor-mount :global(li[data-checked="true"]::before) {
        content: "☑";
    }

    .editor-mount :global(li[data-checked="true"]) {
        color: rgba(0, 0, 0, 0.55);
    }

    .editor-mount :global(code) {
        background: rgba(0, 0, 0, 0.06);
        border-radius: 4px;
        padding: 0.1em 0.3em;
        font-size: 0.9em;
    }

    .editor-mount :global(pre) {
        font-family: ui-monospace, Menlo, monospace;
        font-size: 0.9em;
        background: rgba(0, 0, 0, 0.06);
        border-radius: 8px;
        padding: 0.75rem 1rem;
        overflow-x: auto;
    }

    .editor-mount :global(pre code) {
        background: transparent;
        padding: 0;
    }

    @media (prefers-color-scheme: dark) {
        .editor-mount :global(code),
        .editor-mount :global(pre) {
            background: rgba(255, 255, 255, 0.1);
        }

        .editor-mount :global(li[data-checked="true"]) {
            color: rgba(255, 255, 255, 0.55);
        }
    }

    :global(.editor-mount div.attachment) {
        display: flex;
        align-items: center;
        gap: 0.6rem;
        flex-wrap: wrap;
        margin: 0.4rem 0;
    }
    :global(.editor-mount div.attachment video) {
        max-width: 100%;
        max-height: 22rem;
    }
    :global(.editor-mount div.attachment audio) {
        flex: 1;
        min-width: 12rem;
    }
    :global(.editor-mount img[data-blocked-image]) {
        display: inline-block;
        width: 1.2em;
        height: 1.2em;
        background: #eee;
    }
</style>
