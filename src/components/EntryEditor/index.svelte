<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onMount, untrack } from 'svelte';
    import { createEditor } from '@/editor/index.js';
    import { classifyHref, parseEntryTarget } from '@/lib/entry-target.js';
    import EditorToolbar from '@/components/EditorToolbar/index.svelte';
    import Button from '@/components/Button/index.svelte';
    import IconLeft from '@/icons/arrow-left.svelte';
    import Input from '@/components/Input/index.svelte';
    import {
        ENTRY_EDITOR_TEXT,
        LINK_POPOVER_ACTIONS,
        MOOD_OPTIONS,
        attachmentLabel,
        loadProblemText,
        moodChipLabel,
        tagsChipLabel,
    } from '@/containers/EntryEditorContainer/data.js';

    let {
        // The loaded entry (null while opening); load states below.
        entry = null,
        loadFailed = false,
        // 'deleted' | 'missing' | 'error'.
        loadProblem = null,
        saveConflict = false,
        frozen = false,
        status = '',
        // The session-owned draft: values in, intents out.
        title = '',
        entryDate = '',
        mood = '',
        tags = [],
        // The body the engine starts from; later edits flow
        // view -> session through onBodyChange and never back.
        initialBody = '',
        navError = '',
        navigating = false,
        linkNote = '',
        deleteError = null,
        pickerOpen = false,
        // Applied once on mount: { sel: { from, to } }.
        navRestore = null,
        canGoBack = false,
        canGoForward = false,
        onBack = null,
        onNavBack = null,
        onNavForward = null,
        // Rendered in place: the container's link panels and picker.
        panels = null,
        picker = null,
        // Session-owned draft: field edits and body changes.
        onField = () => {},
        onBodyChange = () => {},
        // Session-owned workflows: persistence, uploads, vault I/O.
        attachFiles = () => {},
        settleForNavigation = async () => true,
        closeSaves = () => {},
        deleteEntry = async () => true,
        fetchAttachmentMeta = async () => {
            throw new Error('no entry service');
        },
        attachmentUrlFor = () => '',
        openAttachment = () => {},
        openExternalLink = () => {},
        openHref = () => {},
        onLinkEntry = () => {},
        onLinkAction = () => {},
        onRequestBracketPicker = () => {},
        onRetryNav = () => {},
    } = $props();

    let deleteArmed = $state(false);
    let fileInput = $state(null);
    let info = $state.raw(null);
    let editor = $state.raw(null);
    let paneEl = $state(null);
    const titleField = { name: 'title', placeholder: ENTRY_EDITOR_TEXT.titlePlaceholder };

    // Lock freeze reaches the engine here: the container owns the
    // frozen flag, the view applies it to its editor and popovers.
    $effect(() => {
        editor?.setReadonly?.(frozen);
        if (frozen) openPop = null;
    });

    /** Hands an uploaded attachment to the engine for display. */
    export function insertAttachment(info) {
        editor?.insertAttachment(info);
    }

    /** Lets the container settle IME before gating a switch. */
    export function finishComposing() {
        editor?.finishComposing();
    }

    export function getMarkdown() {
        return editor ? editor.getMarkdown() : null;
    }

    export function query() {
        return editor ? editor.query() : null;
    }

    export function exec(name, arg) {
        return editor?.exec(name, arg);
    }

    export function focus() {
        editor?.focus();
    }

    export function readText(from, to) {
        return editor ? editor.readText(from, to) : '';
    }

    export function locateLink(href, hintPos) {
        return editor ? editor.locateLink(href, hintPos) : null;
    }

    /* ---- Compact meta chips: one popover open at a time. ---- */
    let openPop = $state(null);

    function togglePop(name) {
        openPop = openPop === name ? null : name;
    }

    function setTags(next) {
        onField('tagsText', next.join(', '));
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
        if (!d) return ENTRY_EDITOR_TEXT.dateChipFallback;
        const dt = new Date(`${d}T00:00:00`);
        if (Number.isNaN(dt.getTime())) return d;
        return dt.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
    }

    function onTitleInput(e) {
        onField('title', e.currentTarget.value);
    }

    function onDateChange(e) {
        onField('entryDate', e.currentTarget.value);
    }

    function setMood(value) {
        onField('mood', value);
    }

    function refreshToolbar() {
        info = editor ? editor.query() : null;
    }

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
                markdown: initialBody,
                onChange: (markdown) => onBodyChange(markdown),
                onSelectionChange: refreshToolbar,
                onAttach: (file) => attachFiles([file]),
                onAttachmentView: (el, id) => hydrateAttachment(el, id),
                onOpenLink: (href, modified) => openHref(href, modified),
                onRequestLinkPicker: ({ bracketPos }) => onRequestBracketPicker(bracketPos),
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
                label.textContent = attachmentLabel(meta.name, meta.size);
                el.appendChild(label);
            }
            el.title = ENTRY_EDITOR_TEXT.doubleClickToOpen;
            el.addEventListener('dblclick', () => {
                openAttachment(meta.id);
            });
        } catch {
            el.textContent = ENTRY_EDITOR_TEXT.attachmentUnavailable;
        }
    }

    /** The current selection, for history restores. */
    export function navCapture() {
        if (!editor) return null;
        const sel = editor.query();
        return { from: sel.from, to: sel.to };
    }

    function popoverAction(id, link) {
        onLinkAction(id, link);
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

    const linkInfo = $derived(!frozen && !pickerOpen && info?.link ? info.link : null);
    const linkKind = $derived(linkInfo ? classifyHref(linkInfo.href) : null);
    const showPopover = $derived(linkInfo !== null && (linkKind === 'entry' || linkKind === 'external'));

    /* Every departure settles through the same session gate; the
       view stays on the entry when it reports failure. */
    async function goBack() {
        if (frozen) return;
        if (await settleForNavigation()) onBack?.();
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
            <Button kind="quiet" handler={goBack} aria-label={ENTRY_EDITOR_TEXT.back}
                ><IconLeft />{ENTRY_EDITOR_TEXT.back}</Button>
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
                            <h4>{ENTRY_EDITOR_TEXT.dateHeading}</h4>
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
                        {moodChipLabel(mood)}
                        <span class="chev">▾</span>
                    </button>
                    {#if openPop === 'mood'}
                        <div class="popover">
                            <h4>{ENTRY_EDITOR_TEXT.moodHeading}</h4>
                            <div class="mood-grid">
                                {#each MOOD_OPTIONS as option (option.value)}
                                    <button
                                        class:selected={mood === option.value}
                                        onclick={() => {
                                            setMood(option.value);
                                            openPop = null;
                                        }}>{option.label}</button>
                                {/each}
                            </div>
                            {#if mood !== ''}
                                <button class="mood-clear" onclick={() => setMood('')}
                                    >{ENTRY_EDITOR_TEXT.clearMood}</button>
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
                        {tagsChipLabel(tags)}
                        <span class="chev">▾</span>
                    </button>
                    {#if openPop === 'tags'}
                        <div class="popover">
                            <h4>{ENTRY_EDITOR_TEXT.tagsHeading}</h4>
                            {#if tags.length > 0}
                                <div class="tag-flow">
                                    {#each tags as tag}
                                        <span class="tag-pill">
                                            <span class="tag-name">{tag}</span>
                                            <button
                                                class="tag-remove"
                                                title={ENTRY_EDITOR_TEXT.removeTagLabel}
                                                onclick={() => removeTag(tag)}>×</button>
                                        </span>
                                    {/each}
                                </div>
                            {/if}
                            <input
                                class="tag-entry"
                                placeholder={ENTRY_EDITOR_TEXT.addTagPlaceholder}
                                onkeydown={addTag} />
                        </div>
                    {/if}
                </div>
            </div>
        </div>

        <Input field={titleField} class="title-input" value={title} oninput={onTitleInput} disabled={frozen} />
        <EditorToolbar
            selection={info}
            disabled={frozen}
            onCommand={(name, arg) => editor?.exec(name, arg)}
            onAttach={() => fileInput?.click()}
            onLinkEntry={onLinkEntry} />
        <input type="file" multiple hidden bind:this={fileInput} onchange={onFilesPicked} />
        <div class="editor-mount" {@attach mountEditor}></div>
        {@render panels?.()}
        {#if showPopover}
            {@const targetId = parseEntryTarget(linkInfo.href)}
            <div
                class="link-popover"
                style={popoverStyle(linkInfo)}
                role="dialog"
                aria-label={ENTRY_EDITOR_TEXT.linkActionsLabel}>
                {#if linkKind === 'entry' && targetId}
                    {#each LINK_POPOVER_ACTIONS as action (action.id)}
                        <button
                            class="pop"
                            class:primary={action.primary}
                            onmousedown={keepPopoverFocus}
                            onclick={() => popoverAction(action.id, linkInfo)}>
                            {action.label}
                        </button>
                    {/each}
                {:else if linkKind === 'external'}
                    <button
                        class="pop primary"
                        onmousedown={keepPopoverFocus}
                        onclick={() => openExternalLink(linkInfo.href)}>
                        {ENTRY_EDITOR_TEXT.openExternalLink}
                    </button>
                {/if}
                {#if linkNote}<span class="pop-note">{linkNote}</span>{/if}
            </div>
        {/if}
        {@render picker?.()}
        {#if saveConflict}
            <p class="note conflict" role="alert">
                {ENTRY_EDITOR_TEXT.conflictMessage}
            </p>
        {/if}
        {#if navError}
            <p class="note nav-error" role="alert">
                {navError}
                <button class="mini" onclick={() => onRetryNav()} disabled={navigating}>
                    {ENTRY_EDITOR_TEXT.retry}
                </button>
            </p>
        {/if}
        {#if deleteError}
            <p class="note nav-error" role="alert">
                {deleteError}
            </p>
        {/if}
        <div class="editor-foot">
            <Button kind="quiet" handler={goBack} aria-label={ENTRY_EDITOR_TEXT.back}
                ><IconLeft />{ENTRY_EDITOR_TEXT.back}</Button>
            {#if canGoBack || canGoForward}
                <Button
                    kind="quiet"
                    handler={() => onNavBack?.()}
                    aria-label={ENTRY_EDITOR_TEXT.goBackLabel}
                    title={ENTRY_EDITOR_TEXT.goBackTitle}
                    disabled={!canGoBack || navigating}>←</Button>
                <Button
                    kind="quiet"
                    handler={() => onNavForward?.()}
                    aria-label={ENTRY_EDITOR_TEXT.goForwardLabel}
                    title={ENTRY_EDITOR_TEXT.goForwardTitle}
                    disabled={!canGoForward || navigating}>→</Button>
            {/if}
            <span class="status">{saveConflict ? ENTRY_EDITOR_TEXT.conflictStatus : status}</span>
            {#if deleteArmed}
                <Button kind="danger" class="confirm-button" handler={remove}
                    >{ENTRY_EDITOR_TEXT.confirmDelete}</Button>
                <Button kind="quiet" class="keep-button" handler={() => (deleteArmed = false)}
                    >{ENTRY_EDITOR_TEXT.keepEntry}</Button>
            {:else}
                <Button kind="quiet" class="delete-button" handler={() => (deleteArmed = true)}
                    >{ENTRY_EDITOR_TEXT.deleteEntry}</Button>
            {/if}
        </div>
    </div>
{:else if loadFailed}
    <p class="note" role="status">
        {loadProblemText(loadProblem)}
    </p>
    <Button kind="quiet" handler={() => onBack?.()} aria-label={ENTRY_EDITOR_TEXT.back}
        ><IconLeft />{ENTRY_EDITOR_TEXT.back}</Button>
{:else}
    <p class="note">{ENTRY_EDITOR_TEXT.opening}</p>
{/if}

<style src="./styles.css"></style>
