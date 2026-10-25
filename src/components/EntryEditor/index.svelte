<script>
    import { onMount, untrack } from 'svelte';
    import * as api from '@/api.js';
    import { createEditor } from '@/editor/index.js';
    import EditorToolbar from '@/components/EditorToolbar/index.svelte';
    import Button from '@/components/Button/index.svelte';
    import IconLeft from '@/icons/arrow-left.svelte';
    import Input from '@/components/Input/index.svelte';
    import { createAutosave } from '@/lib/autosave.svelte.js';

    let { entryId, onDeleted, onSaved, onBack } = $props();

    let entry = $state(null);
    let loadFailed = $state(false);
    let title = $state('');
    let entryDate = $state('');
    let mood = $state('');
    let tagsText = $state('');
    let deleteArmed = $state(false);
    let fileInput = $state(null);
    let info = $state.raw(null);
    let editor = $state.raw(null);
    const titleField = { name: 'title', placeholder: 'Title' };

    /* Tags are edited as comma-separated text but sent as an array. */
    const tags = $derived(
        tagsText
            .split(',')
            .map((t) => t.trim())
            .filter((t) => t !== ''),
    );

    /* ---- Autosave: owns the timer, revisions and single-flight sending. ---- */
    const autosave = createAutosave({
        snapshot: () =>
            entry && editor
                ? {
                      id: entry.id,
                      entry_date: entryDate,
                      title: title.trim() === '' ? null : title.trim(),
                      body_md: editor.getMarkdown(),
                      mood: mood === '' ? null : Number(mood),
                      tz: entry.tz,
                      tags: [...tags],
                  }
                : null,
        send: async (payload) => {
            entry = await api.entrySave(payload);
            onSaved?.();
        },
    });

    /* ---- Attachment progress: facts in state, text derived. ---- */
    // Matches the backend's MAX_ACTIVE_UPLOADS safety limit.
    const MAX_CONCURRENT_UPLOADS = 8;
    const attachmentQueue = [];
    let activeAttachments = 0;
    let pendingAttachments = $state(0);
    let attachmentFailures = $state.raw([]);

    const attachmentNote = $derived(
        pendingAttachments > 0
            ? `Attaching ${pendingAttachments} file${pendingAttachments === 1 ? '' : 's'}…`
            : attachmentFailures.length === 1
              ? attachmentFailures[0]
              : attachmentFailures.length > 1
                ? `${attachmentFailures.length} files could not be attached. ${attachmentFailures[0]}`
                : '',
    );

    const SAVE_LABEL = {
        idle: '',
        dirty: 'Not saved yet',
        saving: 'Saving…',
        saved: 'Saved',
        error: 'Could not save',
    };
    const status = $derived(attachmentNote || SAVE_LABEL[autosave.state]);

    /* ---- Compact meta chips: one popover open at a time. ---- */
    let openPop = $state(null);

    function togglePop(name) {
        openPop = openPop === name ? null : name;
    }

    function setTags(next) {
        tagsText = next.join(', ');
        autosave.touch();
    }

    function removeTag(name) {
        setTags(tags.filter((t) => t !== name));
    }

    function addTag(e) {
        if (e.key !== 'Enter' && e.key !== ',') return;
        const values = e.currentTarget.value
            .split(',')
            .map((t) => t.trim())
            .filter((t) => t !== '');
        if (values.length === 0) return;
        e.preventDefault();
        e.currentTarget.value = '';
        const known = new Set(tags.map((t) => t.toLowerCase()));
        const fresh = [];
        for (const value of values) {
            const key = value.toLowerCase();
            if (known.has(key)) continue;
            known.add(key);
            fresh.push(value);
        }
        if (fresh.length > 0) setTags([...tags, ...fresh]);
    }

    /* Chip label: "Wed, Oct 20" instead of "2026-10-20". */
    function formatDate(d) {
        if (!d) return 'Date';
        const dt = new Date(`${d}T00:00:00`);
        if (Number.isNaN(dt.getTime())) return d;
        return dt.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
    }

    function onTitleInput(e) {
        title = e.currentTarget.value;
        autosave.touch();
    }

    function onDateChange(e) {
        entryDate = e.currentTarget.value;
        autosave.touch();
    }

    function setMood(value) {
        mood = value;
        autosave.touch();
    }

    function refreshToolbar() {
        info = editor ? editor.query() : null;
    }

    onMount(async () => {
        try {
            entry = await api.entryGet(entryId);
        } catch {
            loadFailed = true;
            return;
        }
        title = entry.title ?? '';
        entryDate = entry.entry_date;
        mood = entry.mood == null ? '' : String(entry.mood);
        tagsText = entry.tags.join(', ');
    });

    /* Clicking outside any chip closes its popover. Popovers live
       inside .chip-wrap, so clicks on them are "inside" and stay. */
    onMount(() => {
        const onDocClick = (e) => {
            if (e.target.closest('.chip-wrap')) return;
            openPop = null;
        };
        document.addEventListener('click', onDocClick);
        return () => document.removeEventListener('click', onDocClick);
    });

    const ASSET_PREFIXES = ['soirvu-asset://localhost/', 'http://soirvu-asset.localhost/'];

    /* Attachment on the mount div: runs once when the element is
       created and returns its own teardown. createEditor is untracked
       so reads inside it never rebuild the editor. */
    function mountEditor(node) {
        const instance = untrack(() =>
            createEditor(node, {
                markdown: entry.body_md,
                onChange: () => autosave.touch(),
                onSelectionChange: refreshToolbar,
                onAttach: (file) => attachFile(file),
                onAttachmentView: (el, id) => hydrateAttachment(el, id),
                onOpenLink: (href) => {
                    if (href.startsWith('https://') || href.startsWith('mailto:')) {
                        api.openLink(href).catch(() => {});
                    }
                },
            }),
        );
        editor = instance;
        node.addEventListener('dblclick', onEditorDblClick);

        return () => {
            node.removeEventListener('dblclick', onEditorDblClick);
            // Freezes the last snapshot synchronously, so it is safe
            // to destroy the editor right after.
            autosave.close();
            instance.destroy();
            editor = null;
        };
    }

    function onEditorDblClick(e) {
        const src = e.target.closest('img')?.getAttribute('src') || '';
        const prefix = ASSET_PREFIXES.find((p) => src.startsWith(p));
        if (prefix) {
            api.attachmentOpen(src.slice(prefix.length)).catch(() => {});
        }
    }

    const ATTACH_CHUNK = 1024 * 1024;

    function attachFile(file) {
        if (!entry || !editor) return;
        if (pendingAttachments === 0) attachmentFailures = [];
        attachmentQueue.push(file);
        pendingAttachments++;
        drainAttachmentQueue();
    }

    function drainAttachmentQueue() {
        while (activeAttachments < MAX_CONCURRENT_UPLOADS && attachmentQueue.length) {
            const file = attachmentQueue.shift();
            activeAttachments++;
            void uploadFile(file)
                .then((error) => {
                    if (error) attachmentFailures = [...attachmentFailures, error];
                })
                .finally(() => {
                    activeAttachments--;
                    pendingAttachments--;
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
            editor?.insertAttachment({ id: added.id, mime: added.mime, name: file.name });
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
            const meta = await api.attachmentInfo(id);
            el.replaceChildren();
            const url = api.assetUrl(meta.id);
            if (meta.mime.startsWith('video/')) {
                const v = document.createElement('video');
                v.controls = true;
                v.src = url;
                el.appendChild(v);
            } else if (meta.mime.startsWith('audio/')) {
                const a = document.createElement('audio');
                a.controls = true;
                a.src = url;
                el.appendChild(a);
            } else {
                const label = document.createElement('span');
                label.textContent = `${meta.name} · ${Math.max(1, Math.round(meta.size / 1024))} KB`;
                el.appendChild(label);
            }
            el.title = 'Double-click to open';
            el.addEventListener('dblclick', () => {
                api.attachmentOpen(meta.id).catch(() => {});
            });
        } catch {
            el.textContent = 'Attachment unavailable.';
        }
    }

    /* Save first; stay on the entry if the save failed. */
    async function goBack() {
        await autosave.flush();
        if (autosave.clean) onBack?.();
    }

    async function remove() {
        autosave.discard(); // never save an entry that is being deleted
        await api.entryDelete(entry.id);
        onDeleted?.();
    }
</script>

<svelte:window
    onkeydown={(e) => {
        if (e.key === 'Escape') openPop = null;
    }} />

{#if entry}
    <div class="editor-pane">
        <!-- One compact meta row: back + chips. Date, mood and
             tags each open a popover instead of stacking rows. -->
        <div class="meta-row">
            <Button kind="quiet" handler={goBack} aria-label="Back"><IconLeft />Back</Button>
            <div class="meta-chips">
                <div class="chip-wrap">
                    <button
                        class="meta-chip"
                        class:empty={!entryDate}
                        aria-expanded={openPop === 'date'}
                        onclick={() => togglePop('date')}>
                        <span class="chip-icon">📅</span>
                        {formatDate(entryDate)}
                        <span class="chev">▾</span>
                    </button>
                    {#if openPop === 'date'}
                        <div class="popover">
                            <h4>Date</h4>
                            <input type="date" value={entryDate} onchange={onDateChange} />
                        </div>
                    {/if}
                </div>

                <div class="chip-wrap">
                    <button
                        class="meta-chip"
                        class:empty={mood === ''}
                        aria-expanded={openPop === 'mood'}
                        onclick={() => togglePop('mood')}>
                        <span class="chip-icon">★</span>
                        {mood === '' ? 'Mood' : `Mood ${mood}/5`}
                        <span class="chev">▾</span>
                    </button>
                    {#if openPop === 'mood'}
                        <div class="popover">
                            <h4>Mood</h4>
                            <div class="mood-grid">
                                {#each ['1', '2', '3', '4', '5'] as m}
                                    <button
                                        class:selected={mood === m}
                                        onclick={() => {
                                            setMood(m);
                                            openPop = null;
                                        }}>{m}</button>
                                {/each}
                            </div>
                            {#if mood !== ''}
                                <button class="mood-clear" onclick={() => setMood('')}>Clear mood</button>
                            {/if}
                        </div>
                    {/if}
                </div>

                <div class="chip-wrap">
                    <button
                        class="meta-chip"
                        class:empty={tags.length === 0}
                        aria-expanded={openPop === 'tags'}
                        onclick={() => togglePop('tags')}>
                        <span class="chip-icon">#</span>
                        {tags.length === 0
                            ? 'Tags'
                            : `${tags.slice(0, 2).join(', ')}${tags.length > 2 ? ` +${tags.length - 2}` : ''}`}
                        <span class="chev">▾</span>
                    </button>
                    {#if openPop === 'tags'}
                        <div class="popover">
                            <h4>Tags</h4>
                            {#if tags.length > 0}
                                <div class="tag-flow">
                                    {#each tags as tag}
                                        <span class="tag-pill">
                                            <span class="tag-name">{tag}</span>
                                            <button class="tag-remove" title="Remove tag" onclick={() => removeTag(tag)}
                                                >×</button>
                                        </span>
                                    {/each}
                                </div>
                            {/if}
                            <input class="tag-entry" placeholder="Add tag… (Enter)" onkeydown={addTag} />
                        </div>
                    {/if}
                </div>
            </div>
        </div>

        <Input field={titleField} class="title-input" value={title} oninput={onTitleInput} />
        <EditorToolbar {editor} {info} onAttach={() => fileInput?.click()} />
        <input type="file" multiple hidden bind:this={fileInput} onchange={onFilesPicked} />
        <div class="editor-mount" {@attach mountEditor}></div>
        <div class="editor-foot">
            <Button kind="quiet" handler={goBack} aria-label="Back"><IconLeft />Back</Button>
            <span class="status">{status}</span>
            {#if deleteArmed}
                <Button kind="danger" class="confirm-button" handler={remove}>Really delete entry?</Button>
                <Button kind="quiet" class="keep-button" handler={() => (deleteArmed = false)}>Keep it</Button>
            {:else}
                <Button kind="quiet" class="delete-button" handler={() => (deleteArmed = true)}>Delete entry</Button>
            {/if}
        </div>
    </div>
{:else}
    <p class="note">{loadFailed ? 'Could not open this entry.' : 'Opening…'}</p>
{/if}

<style src="./styles.css"></style>
