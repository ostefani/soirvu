<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy, onMount, untrack } from 'svelte';
    import * as api from '@/api.js';
    import { createEditor } from '@/editor/index.js';
    import { createRequestOwner } from '@/lib/request-owner.js';
    import { classifyHref, parseEntryTarget } from '@/lib/links.js';
    import EditorToolbar from '@/components/EditorToolbar/index.svelte';
    import LinkPanels from '@/components/LinkPanels/index.svelte';
    import LinkPicker from '@/components/LinkPicker/index.svelte';
    import Button from '@/components/Button/index.svelte';
    import IconLeft from '@/icons/arrow-left.svelte';
    import Input from '@/components/Input/index.svelte';
    import { createAutosave } from '@/lib/autosave.svelte.js';
    import { createEntryCommit, mayFinishAfterFlush } from '@/lib/entry-commit.js';
    import { registerFlushHook } from '@/lib/lock-hooks.js';
    import { notify } from '@/components/Notification/store.svelte.js';
    import { EVENT_TYPE } from '@/constants.js';

    let {
        entryId,
        onDeleted,
        onSaved,
        onBack,
        folders = [],
        // Navigating to another entry (link follow, history).
        onOpenEntry = null,
        canGoBack = false,
        canGoForward = false,
        onNavBack = null,
        onNavForward = null,
        // Applied once on mount: { sel: { from, to } }.
        navRestore = null,
        // Workspace epoch (import/sync changes): forwarded to the
        // link panels so visible backlink/metadata refresh without
        // touching the loaded draft.
        workspaceTick = 0,
    } = $props();

    let entry = $state(null);
    let loadFailed = $state(false);
    // A deleted or unknown navigation target keeps its notice
    // instead of the editor: 'deleted' | 'missing' | 'error'.
    let loadProblem = $state(null);
    let navigating = $state(false);
    let navError = $state('');
    let pendingNavId = $state(null);
    // The picker overlay: { bracketPos: number | null, initialText }.
    let linkPicker = $state(null);
    let popoverNote = $state('');
    let paneEl = $state(null);
    let title = $state('');
    let entryDate = $state('');
    let mood = $state('');
    let tagsText = $state('');
    let deleteArmed = $state(false);
    let frozen = $state(false);
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

    /* Sticky once set: the open rev is behind the stored one, so
       every later save with it fails too. Local edits are kept;
       reopening the entry remounts with the current rev. */
    let saveConflict = $state(false);

    /* ---- Autosave: owns the timer, revisions and single-flight sending. ---- */
    // The snapshot builder lives here so a rebased resend can take
    // a fresh one; the helper only schedules sends.
    function takeSnapshot() {
        return entry && editor
            ? {
                  entry_date: entryDate,
                  title: title.trim() === '' ? null : title.trim(),
                  body_md: editor.getMarkdown(),
                  mood: mood === '' ? null : Number(mood),
                  tz: entry.tz,
                  tags: [...tags],
              }
            : null;
    }

    /* One entry's commit sequence: content saves and attachment
       completions run exclusively, in submission order, each with
       the newest committed base. Our own operations can no longer
       race each other, so a conflict here is genuinely foreign —
       surface it with the draft preserved instead of retrying. */
    let commitSeq = null;

    // The initial entry load: disposal (navigation, lock) retires
    // it, so a late response cannot publish into a torn-down pane.
    const loadOwner = createRequestOwner();

    onDestroy(() => loadOwner.dispose());

    const autosave = createAutosave({
        snapshot: takeSnapshot,
        send: async (payload) => {
            try {
                entry = await commitSeq.commit((base) =>
                    api.entryUpdate(entry.id, base, payload),
                );
            } catch (e) {
                if (e === 'revision_conflict') saveConflict = true;
                throw e;
            }
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
    let abortUploads = false;
    const inflightUploads = new Set();

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
        const token = loadOwner.begin();
        try {
            const loaded = await api.entryGet(entryId);
            if (!loadOwner.current(token)) return;
            entry = loaded;
            commitSeq = createEntryCommit(entry.rev);
        } catch (e) {
            if (!loadOwner.current(token)) return;
            loadFailed = true;
            // A followed link whose target is gone keeps its notice
            // instead of a bare load failure.
            if (e === 'entry_not_found') loadProblem = 'deleted';
            else if (e === 'invalid_input') loadProblem = 'missing';
            else loadProblem = 'error';
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

    onMount(() =>
        registerFlushHook(async () => {
            frozen = true;
            editor?.setReadonly?.(true);
            openPop = null;
            await abortAllUploads();
            await autosave.flush();
            const ok = autosave.clean && pendingAttachments === 0;
            if (!ok) {
                editor?.setReadonly?.(false);
                abortUploads = false;
                frozen = false;
                notify(EVENT_TYPE.ERROR, 'Latest edits could not be saved.');
            }
            return ok;
        }),
    );

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
                onOpenLink: (href, modified) => routeLink(href, modified),
                onRequestLinkPicker: ({ bracketPos }) => {
                    if (!frozen) linkPicker = { bracketPos, initialText: '' };
                },
            }),
        );
        editor = instance;
        if (navRestore?.sel) {
            try {
                editor.setTextSelection(navRestore.sel.from, navRestore.sel.to);
            } catch {
                // A changed document keeps the default top selection.
            }
        }
        node.addEventListener('dblclick', onEditorDblClick);

        return () => {
            node.removeEventListener('dblclick', onEditorDblClick);
            // Freezes the last snapshot synchronously, so it is safe
            // to destroy the editor right after.
            autosave.close().then((ok) => {
                if (!ok) notify(EVENT_TYPE.ERROR, 'Latest edits could not be saved.');
            });
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

    async function abortAllUploads() {
        abortUploads = true;
        const queued = attachmentQueue.length;
        attachmentQueue.length = 0;
        pendingAttachments -= queued;
        await Promise.allSettled([...inflightUploads]);
    }

    function attachFile(file) {
        if (!entry || !editor || frozen) return;
        if (pendingAttachments === 0) attachmentFailures = [];
        attachmentQueue.push(file);
        pendingAttachments++;
        drainAttachmentQueue();
    }

    function drainAttachmentQueue() {
        if (abortUploads) return;
        while (activeAttachments < MAX_CONCURRENT_UPLOADS && attachmentQueue.length) {
            const file = attachmentQueue.shift();
            activeAttachments++;
            const done = uploadFile(file)
                .then((error) => {
                    if (error) attachmentFailures = [...attachmentFailures, error];
                })
                .finally(() => {
                    activeAttachments--;
                    pendingAttachments--;
                    inflightUploads.delete(done);
                    drainAttachmentQueue();
                });
            inflightUploads.add(done);
        }
    }

    async function uploadFile(file) {
        let uploadId = null;
        try {
            uploadId = await api.attachmentBegin(entry.id, file.name, file.type || null);
            for (let offset = 0; offset < file.size; offset += ATTACH_CHUNK) {
                if (abortUploads) {
                    await api.attachmentAbort(uploadId).catch(() => {});
                    return null;
                }
                const slice = await file.slice(offset, offset + ATTACH_CHUNK).arrayBuffer();
                await api.attachmentChunk(uploadId, new Uint8Array(slice));
            }
            // Settle our own saves first, so the attachment lands on
            // saved text. A flush that leaves the draft unsaved
            // through a FAILED save stops the finish: proceeding
            // would stack a write under text that is already behind
            // (and possibly foreign). Fresh mid-flush edits are
            // fine — the sequence still gives this finish a new base.
            const saved = await autosave.flush();
            if (!mayFinishAfterFlush(saved, autosave.state)) {
                await api.attachmentAbort(uploadId).catch(() => {});
                uploadId = null;
                return `${file.name}: latest edits could not be saved — attach again after saving.`;
            }
            const added = await commitSeq.commit((base) =>
                api.attachmentFinish(uploadId, base),
            );
            uploadId = null;
            // The commit bumped the entry rev server-side; adopt it
            // before dirtying the document so the next save builds
            // on the fresh base.
            if (entry) entry = { ...entry, rev: added.rev };
            editor?.insertAttachment({ id: added.id, mime: added.mime, name: file.name });
            return null;
        } catch (e) {
            if (uploadId) await api.attachmentAbort(uploadId).catch(() => {});
            if (e === 'revision_conflict') {
                // Sync (or another writer) moved the entry mid-upload:
                // the commit was refused, so there is nothing to adopt.
                // Surface the standard conflict instead of clobbering
                // unseen content with the next save.
                saveConflict = true;
                return `${file.name}: the entry changed elsewhere — reopen it and attach again.`;
            }
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

    /* ---- Entry links (Step 8): authoring, popover, navigation. ---- */

    function defaultLinkLabel(target) {
        return target.title || `Untitled — ${target.entry_date}`;
    }

    function openLinkPicker() {
        if (frozen || !editor) return;
        // The selection stays the label only. Prefilling the search
        // with it dead-ends whenever the label matches nothing (the
        // picker would open on "No entries match" with the input
        // buried); recent entries are the better starting list, and
        // the input is already focused for filtering.
        linkPicker = { bracketPos: null, initialText: '' };
    }

    function cancelPicker() {
        linkPicker = null;
        editor?.focus();
    }

    function pickLinkTarget(target) {
        const picker = linkPicker;
        if (!editor || !picker) return;
        linkPicker = null;
        let from;
        let to;
        let label;
        if (picker.bracketPos != null) {
            from = picker.bracketPos;
            to = picker.bracketPos + 2;
            label = defaultLinkLabel(target);
        } else {
            const sel = editor.query();
            from = sel.from;
            to = sel.to;
            label = sel.empty ? defaultLinkLabel(target) : editor.readText(from, to);
        }
        if (!label.trim()) {
            editor.focus();
            return;
        }
        editor.exec('insertEntryLink', { targetId: target.id, label, from, to });
        editor.focus();
    }

    function routeLink(href, modified) {
        if (!modified || frozen) return;
        const kind = classifyHref(href);
        if (kind === 'entry') {
            const id = parseEntryTarget(href);
            if (id) openEntry(id);
        } else if (kind === 'external') {
            api.openLink(href).catch(() => {});
        }
    }

    async function openEntry(id) {
        if (navigating || frozen || !entry) return;
        navigating = true;
        navError = '';
        try {
            editor?.finishComposing();
            // Settle, never abort: an upload landing after the switch
            // would orphan its attachment from the saved old entry.
            await Promise.allSettled([...inflightUploads]);
            await autosave.flush();
            if (!autosave.clean || pendingAttachments > 0) {
                pendingNavId = id;
                navError = 'Latest edits could not be saved. Retry to navigate.';
                return;
            }
            pendingNavId = null;
            const sel = editor?.query();
            onOpenEntry?.({ id, selFrom: sel?.from ?? 0, selTo: sel?.to ?? 0 });
        } finally {
            navigating = false;
        }
    }

    /** Settles uploads and flushes for container-gated switches
     *  (list, search results, history). False stays put: the foot
     *  already shows the save state. */
    export async function flushForNavigation() {
        if (!entry || !editor || frozen) return true;
        editor.finishComposing();
        await Promise.allSettled([...inflightUploads]);
        await autosave.flush();
        return autosave.clean && pendingAttachments === 0;
    }

    /** The current selection, for history restores. */
    export function navCapture() {
        if (!editor) return null;
        const sel = editor.query();
        return { from: sel.from, to: sel.to };
    }

    /* The popover anchors a range captured at render; after any
       await (or any edit) that range can be stale, so every action
       re-resolves the run by href first and bails out with a note
       instead of touching the wrong text. */
    function liveLinkRange(link) {
        const live = editor?.locateLink(link.href, link.from) ?? null;
        if (!live) popoverNote = 'The link moved — place the cursor on it and retry.';
        return live;
    }

    function removeLinkMark(link) {
        const live = liveLinkRange(link);
        if (live) editor.exec('removeLink', live);
    }

    async function copyInternalLink(link) {
        const live = liveLinkRange(link);
        if (!live) return;
        const label = editor.readText(live.from, live.to);
        try {
            await copyText(`[${label}](${link.href})`);
            await api.clipboardNoteCopy();
            popoverNote = 'Internal link copied.';
        } catch {
            popoverNote = 'Could not copy the link.';
        }
    }

    async function copyText(text) {
        try {
            await navigator.clipboard.writeText(text);
            return;
        } catch {
            // The webview may not grant the async clipboard path.
        }
        const area = document.createElement('textarea');
        area.value = text;
        area.style.position = 'fixed';
        area.style.opacity = '0';
        document.body.appendChild(area);
        area.select();
        try {
            if (!document.execCommand('copy')) throw new Error('copy refused');
        } finally {
            area.remove();
        }
    }

    async function refreshLinkLabel(link) {
        const id = parseEntryTarget(link.href);
        if (!id || !editor) return;
        try {
            const target = await api.entryGet(id);
            // The fetch crossed an await: re-resolve the run instead
            // of writing to the captured range.
            const live = liveLinkRange(link);
            if (!live) return;
            editor.exec('insertEntryLink', {
                targetId: id,
                label: defaultLinkLabel(target),
                from: live.from,
                to: live.to,
            });
            popoverNote = '';
        } catch (e) {
            popoverNote =
                e === 'entry_not_found'
                    ? 'The target entry was deleted.'
                    : `Could not read the target (${e}).`;
        }
    }

    function popoverStyle(link) {
        if (!editor || !paneEl) return '';
        try {
            const rect = editor.coordsAtPos(link.from);
            const pane = paneEl.getBoundingClientRect();
            const left = Math.max(8, Math.min(rect.left - pane.left, pane.width - 280));
            const top = rect.bottom - pane.top + 6;
            return `left: ${left}px; top: ${top}px;`;
        } catch {
            return '';
        }
    }

    function keepPopoverFocus(e) {
        // Clicking a popover button must not move the caret: the
        // popover anchors to the selection it would collapse.
        e.preventDefault();
    }

    const linkInfo = $derived(!frozen && !linkPicker && info?.link ? info.link : null);
    const linkKind = $derived(linkInfo ? classifyHref(linkInfo.href) : null);
    const showPopover = $derived(linkInfo !== null && (linkKind === 'entry' || linkKind === 'external'));

    /* Save first; stay on the entry if the save failed. */
    async function goBack() {
        if (frozen) return;
        await autosave.flush();
        if (autosave.clean) onBack?.();
    }

    async function remove() {
        if (frozen) return;
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
    <div class="editor-pane" bind:this={paneEl}>
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
                        disabled={frozen}
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
                        disabled={frozen}
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
                        disabled={frozen}
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

        <Input field={titleField} class="title-input" value={title} oninput={onTitleInput} disabled={frozen} />
        <EditorToolbar
            {editor}
            {info}
            disabled={frozen}
            onAttach={() => fileInput?.click()}
            onLinkEntry={openLinkPicker} />
        <input type="file" multiple hidden bind:this={fileInput} onchange={onFilesPicked} />
        <div class="editor-mount" {@attach mountEditor}></div>
        <LinkPanels
            entryId={entry.id}
            rev={entry.rev}
            readBody={() => editor?.getMarkdown() ?? entry.body_md}
            {folders}
            workspaceTick={workspaceTick}
            onOpenEntry={(id) => openEntry(id)}
            onRemoveLink={({ from, to }) => editor?.exec('removeLink', { from, to })} />
        {#if showPopover}
            {@const targetId = parseEntryTarget(linkInfo.href)}
            <div class="link-popover" style={popoverStyle(linkInfo)} role="dialog" aria-label="Link actions">
                {#if linkKind === 'entry' && targetId}
                    <button class="pop primary" onmousedown={keepPopoverFocus} onclick={() => openEntry(targetId)}>
                        Open entry
                    </button>
                    <button class="pop" onmousedown={keepPopoverFocus} onclick={() => copyInternalLink(linkInfo)}>
                        Copy link
                    </button>
                    <button
                        class="pop"
                        onmousedown={keepPopoverFocus}
                        onclick={() => refreshLinkLabel(linkInfo)}>
                        Use current title
                    </button>
                    <button class="pop" onmousedown={keepPopoverFocus} onclick={() => removeLinkMark(linkInfo)}>
                        Remove link
                    </button>
                {:else if linkKind === 'external'}
                    <button
                        class="pop primary"
                        onmousedown={keepPopoverFocus}
                        onclick={() => api.openLink(linkInfo.href).catch(() => {})}>
                        Open link
                    </button>
                {/if}
                {#if popoverNote}<span class="pop-note">{popoverNote}</span>{/if}
            </div>
        {/if}
        {#if linkPicker}
            <div
                class="picker-overlay"
                onclick={(e) => {
                    if (e.target === e.currentTarget) cancelPicker();
                }}>
                <LinkPicker
                    initialQuery={linkPicker.initialText}
                    {folders}
                    onPick={pickLinkTarget}
                    onCancel={cancelPicker} />
            </div>
        {/if}
        {#if saveConflict}
            <p class="note conflict" role="alert">
                This entry changed elsewhere — your latest edits are kept but not saved. Reopen the
                entry to merge.
            </p>
        {/if}
        {#if navError}
            <p class="note nav-error" role="alert">
                {navError}
                <button
                    class="mini"
                    onclick={() => pendingNavId && openEntry(pendingNavId)}
                    disabled={navigating}>
                    Retry
                </button>
            </p>
        {/if}
        <div class="editor-foot">
            <Button kind="quiet" handler={goBack} aria-label="Back"><IconLeft />Back</Button>
            {#if canGoBack || canGoForward}
                <Button
                    kind="quiet"
                    handler={() => onNavBack?.()}
                    aria-label="Go back to the previous entry"
                    title="Go back"
                    disabled={!canGoBack || navigating}>←</Button>
                <Button
                    kind="quiet"
                    handler={() => onNavForward?.()}
                    aria-label="Go forward to the next entry"
                    title="Go forward"
                    disabled={!canGoForward || navigating}>→</Button>
            {/if}
            <span class="status">{saveConflict ? 'Not saved — changed elsewhere' : status}</span>
            {#if deleteArmed}
                <Button kind="danger" class="confirm-button" handler={remove}>Really delete entry?</Button>
                <Button kind="quiet" class="keep-button" handler={() => (deleteArmed = false)}>Keep it</Button>
            {:else}
                <Button kind="quiet" class="delete-button" handler={() => (deleteArmed = true)}>Delete entry</Button>
            {/if}
        </div>
    </div>
{:else if loadFailed}
    <p class="note" role="status">
        {loadProblem === 'deleted'
            ? 'This entry was deleted. Its links elsewhere keep their labels.'
            : loadProblem === 'missing'
              ? 'This entry is unavailable.'
              : 'Could not open this entry.'}
    </p>
    <Button kind="quiet" handler={() => onBack?.()} aria-label="Back"><IconLeft />Back</Button>
{:else}
    <p class="note">Opening…</p>
{/if}

<style src="./styles.css"></style>
