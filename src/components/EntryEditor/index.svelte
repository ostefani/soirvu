<script>
    import { onDestroy, onMount } from 'svelte';
    import * as api from '@/api.js';
    import { createEditor } from '@/editor/index.js';
    import EditorToolbar from '@/components/EditorToolbar/index.svelte';

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
    // let editor = null;

    let editor = $state(null);
    let saveTimer = null;
    let dirty = false;
    // Matches the backend's MAX_ACTIVE_UPLOADS safety limit.
    const MAX_CONCURRENT_UPLOADS = 8;
    const attachmentQueue = [];
    let activeAttachments = 0;
    let pendingAttachments = 0;
    let attachmentFailures = [];

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

    const ASSET_PREFIXES = ['soirvu-asset://localhost/', 'http://soirvu-asset.localhost/'];

    $effect(() => {
        if (!entry || !mountEl || editor) return;
        mountEl.addEventListener('dblclick', (e) => {
            const img = e.target.closest('img');
            const src = img?.getAttribute('src') || '';
            const prefix = ASSET_PREFIXES.find((p) => src.startsWith(p));
            if (prefix) {
                api.attachmentOpen(src.slice(prefix.length)).catch(() => {});
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

    const ATTACH_CHUNK = 1024 * 1024;

    function attachFile(file) {
        if (!entry || !editor) return;
        if (pendingAttachments === 0) attachmentFailures = [];
        attachmentQueue.push(file);
        pendingAttachments++;
        status = `Attaching ${pendingAttachments} file${pendingAttachments === 1 ? '' : 's'}…`;
        drainAttachmentQueue();
    }

    function drainAttachmentQueue() {
        while (activeAttachments < MAX_CONCURRENT_UPLOADS && attachmentQueue.length) {
            const file = attachmentQueue.shift();
            activeAttachments++;
            void uploadFile(file)
                .then((error) => {
                    if (error) attachmentFailures.push(error);
                })
                .finally(() => {
                    activeAttachments--;
                    pendingAttachments--;
                    if (pendingAttachments === 0) {
                        status =
                            attachmentFailures.length === 0
                                ? 'Saved'
                                : attachmentFailures.length === 1
                                  ? attachmentFailures[0]
                                  : `${attachmentFailures.length} files could not be attached. ${attachmentFailures[0]}`;
                    } else {
                        status = `Attaching ${pendingAttachments} file${pendingAttachments === 1 ? '' : 's'}…`;
                    }
                    drainAttachmentQueue();
                });
        }
    }

    async function uploadFile(file) {
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
            return null;
        } catch (e) {
            if (uploadId) await api.attachmentAbort(uploadId).catch(() => {});
            if (e === 'too_large') return `${file.name}: that file is too large for its type.`;
            if (e === 'unsupported_type') return `${file.name}: that file type is not supported.`;
            if (e === 'locked') return 'The diary is locked.';
            if (e === 'too_many_uploads') return 'Too many files are being attached. Please try again.';
            return `${file.name}: could not attach the file (${e}).`;
        }
    }

    function onFilesPicked() {
        if (!fileInput) return;
        for (const file of Array.from(fileInput.files || [])) attachFile(file);
        fileInput.value = '';
    }

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
        <input type="file" multiple hidden bind:this={fileInput} onchange={onFilesPicked} />
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

<style src="./styles.css"></style>
