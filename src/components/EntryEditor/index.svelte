<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onMount, untrack } from 'svelte';
    import { createEditor } from '@/editor/index.js';
    import { classifyHref, parseEntryTarget } from '@/lib/links.js';
    import EditorToolbar from '@/components/EditorToolbar/index.svelte';
    import LinkPanelsContainer from '@/containers/LinkPanelsContainer/index.svelte';
    import LinkPickerContainer from '@/containers/LinkPickerContainer/index.svelte';
    import Button from '@/components/Button/index.svelte';
    import IconLeft from '@/icons/arrow-left.svelte';
    import Input from '@/components/Input/index.svelte';

    let {
        // The loaded entry (null while opening); load states below.
        entry = null,
        loadFailed = false,
        // 'deleted' | 'missing' | 'error'.
        loadProblem = null,
        saveConflict = false,
        frozen = false,
        status = '',
        folders = [],
        // Applied once on mount: { sel: { from, to } }.
        navRestore = null,
        // Workspace epoch (import/sync changes): forwarded to the
        // link panels so visible backlink/metadata refresh without
        // touching the loaded draft.
        workspaceTick = 0,
        canGoBack = false,
        canGoForward = false,
        onBack = null,
        // Navigating to another entry (link follow, history).
        onOpenEntry = null,
        onNavBack = null,
        onNavForward = null,
        // Container-owned workflows: persistence, uploads, vault I/O.
        touch = () => {},
        attachFiles = () => {},
        settleForNavigation = async () => true,
        flushSaves = async () => true,
        closeSaves = () => {},
        deleteEntry = async () => {},
        fetchLinkTarget = async () => {
            throw new Error('no entry service');
        },
        noteLinkCopied = async () => false,
        fetchAttachmentMeta = async () => {
            throw new Error('no entry service');
        },
        attachmentUrlFor = () => '',
        openAttachment = () => {},
        openExternalLink = () => {},
    } = $props();

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
    let fileInput = $state(null);
    let info = $state.raw(null);
    let editor = $state.raw(null);
    const titleField = { name: 'title', placeholder: 'Title' };

    // Lock freeze reaches the engine here: the container owns the
    // frozen flag, the view applies it to its editor and popovers.
    $effect(() => {
        editor?.setReadonly?.(frozen);
        if (frozen) openPop = null;
    });

    /* Tags are edited as comma-separated text but sent as an array. */
    const tags = $derived(
        tagsText
            .split(',')
            .map((t) => t.trim())
            .filter((t) => t !== ''),
    );

    /* The draft snapshot for the container's autosave: a rebased
       resend takes a fresh one through this same interface. */
    export function getSnapshot() {
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

    /** Hands an uploaded attachment to the engine for display. */
    export function insertAttachment(info) {
        editor?.insertAttachment(info);
    }

    /** Lets the container settle IME before gating a switch. */
    export function finishComposing() {
        editor?.finishComposing();
    }

    /* ---- Compact meta chips: one popover open at a time. ---- */
    let openPop = $state(null);

    function togglePop(name) {
        openPop = openPop === name ? null : name;
    }

    function setTags(next) {
        tagsText = next.join(', ');
        touch();
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
        touch();
    }

    function onDateChange(e) {
        entryDate = e.currentTarget.value;
        touch();
    }

    function setMood(value) {
        mood = value;
        touch();
    }

    function refreshToolbar() {
        info = editor ? editor.query() : null;
    }

    // The container loads the entry; the fields take their initial
    // values from the first loaded entry only. Later entry updates
    // (saves) must not overwrite what the user is typing.
    let fieldsInit = false;
    $effect(() => {
        if (entry && !fieldsInit) {
            fieldsInit = true;
            title = entry.title ?? '';
            entryDate = entry.entry_date;
            mood = entry.mood == null ? '' : String(entry.mood);
            tagsText = entry.tags.join(', ');
        }
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
                onChange: () => touch(),
                onSelectionChange: refreshToolbar,
                onAttach: (file) => attachFiles([file]),
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
            // The container saves with the engine still alive; only
            // then is it safe to destroy the editor right after.
            closeSaves();
            instance.destroy();
            editor = null;
        };
    }

    function onEditorDblClick(e) {
        const src = e.target.closest('img')?.getAttribute('src') || '';
        const prefix = ASSET_PREFIXES.find((p) => src.startsWith(p));
        if (prefix) {
            openAttachment(src.slice(prefix.length));
        }
    }

    function onFilesPicked() {
        if (!fileInput) return;
        attachFiles(Array.from(fileInput.files || []));
        fileInput.value = '';
    }

    async function hydrateAttachment(el, id) {
        try {
            const meta = await fetchAttachmentMeta(id);
            el.replaceChildren();
            const url = attachmentUrlFor(meta.id);
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
                openAttachment(meta.id);
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
            openExternalLink(href);
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
            if (!(await settleForNavigation())) {
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
            popoverNote = (await noteLinkCopied()) ? 'Internal link copied.' : 'Could not copy the link.';
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
            const target = await fetchLinkTarget(id);
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
        if (await flushSaves()) onBack?.();
    }

    async function remove() {
        if (frozen) return;
        await deleteEntry();
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
        <LinkPanelsContainer
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
                        onclick={() => openExternalLink(linkInfo.href)}>
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
                <LinkPickerContainer
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
