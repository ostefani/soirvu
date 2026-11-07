<script>
    import { onMount } from 'svelte';
    import EditorSurface from './EditorSurface/index.svelte';
    import EntryFooter from './EntryFooter/index.svelte';
    import EntryMeta from './EntryMeta/index.svelte';
    import { classifyHref, parseEntryTarget } from '@/lib/entry-target.js';
    import EditorToolbar from '@/components/EditorToolbar/index.svelte';
    import Button from '@/components/Button/index.svelte';
    import IconLeft from '@/icons/arrow-left.svelte';
    import Input from '@/components/Input/index.svelte';
    import { ENTRY_EDITOR_TEXT } from './data.js';
    import { LINK_POPOVER_ACTIONS } from './constants.js';
    import { loadProblemText } from './labels.js';

    let {
        ready,
        loadFailed,
        loadProblem,
        saveConflict,
        frozen,
        status,
        deleteError,
        draft,
        links,
        attachments,
        history,
        navRestore,
        panels,
        picker,
        onBack,
        onBackFromLoadError,
        onEditorReady,
        onField,
        onBodyChange,
        attachFiles,
        deleteEntry,
    } = $props();

    const pickerOpen = $derived(links.linkPicker !== null);

    let fileInput = $state(null);
    let info = $state.raw(null);
    let editor = $state.raw(null);
    let paneEl = $state(null);
    let linkPopoverEl = $state.raw(null);
    let linkPopoverDismissed = $state(true);
    const titleField = { name: 'title', placeholder: ENTRY_EDITOR_TEXT.titlePlaceholder };

    function trackLinkPopover(node) {
        linkPopoverEl = node;
        return () => { linkPopoverEl = null; };
    }

    function onTitleInput(e) {
        onField('title', e.currentTarget.value);
    }

    onMount(() => {
        function dismissLinkOnOutsideClick(event) {
            if (linkPopoverEl?.contains(event.target)) return;
            const anchor = event.target.closest('.editor-mount a[href]');
            linkPopoverDismissed = !(anchor && paneEl?.contains(anchor));
        }

        document.addEventListener('click', dismissLinkOnOutsideClick, true);
        return () => document.removeEventListener('click', dismissLinkOnOutsideClick, true);
    });

    function registerEditor(instance) {
        editor = instance;
        const detach = onEditorReady(instance);
        return () => {
            detach();
            editor = null;
        };
    }

    function onFilesPicked() {
        if (!fileInput) return;
        attachFiles(Array.from(fileInput.files || []));
        fileInput.value = '';
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
    const showPopover = $derived(
        linkInfo !== null && !linkPopoverDismissed && (linkKind === 'entry' || linkKind === 'external'),
    );

</script>

<svelte:window onkeydown={(event) => {
    if (event.key === 'Escape') linkPopoverDismissed = true;
}} />

{#if ready}
    <div class="editor-pane" bind:this={paneEl}>
        <EntryMeta
            entryDate={draft.entryDate}
            mood={draft.mood}
            tags={draft.tags}
            {frozen}
            {pickerOpen}
            {paneEl}
            {panels}
            copying={links.copying}
            onCopyNoteLink={links.copyNoteLink}
            {onField} />

        <Input field={titleField} class="title-input" value={draft.title} oninput={onTitleInput} disabled={frozen} />
        <EditorToolbar
            selection={info}
            disabled={frozen || pickerOpen}
            onCommand={(name, arg) => editor?.exec(name, arg)}
            onAttach={() => fileInput?.click()}
            onLinkEntry={links.openLinkPicker} />
        <input type="file" multiple hidden bind:this={fileInput} onchange={onFilesPicked} />
        <EditorSurface
            initialMarkdown={draft.body}
            restore={navRestore}
            readonly={frozen}
            inert={pickerOpen}
            {attachments}
            onReady={registerEditor}
            onChange={onBodyChange}
            onSelectionChange={(selection) => (info = selection)}
            onAttachFile={(file) => attachFiles([file])}
            onOpenLink={links.openHref}
            onRequestLinkPicker={links.requestBracketPicker} />
        {#if showPopover}
            {@const targetId = parseEntryTarget(linkInfo.href)}
            <div
                class="link-popover"
                {@attach trackLinkPopover}
                style={popoverStyle(linkInfo)}
                role="dialog"
                aria-label={ENTRY_EDITOR_TEXT.linkActionsLabel}>
                {#if linkKind === 'entry' && targetId}
                    {#each LINK_POPOVER_ACTIONS as action (action.id)}
                        <button
                            class="pop"
                            class:primary={action.primary}
                            onmousedown={keepPopoverFocus}
                            onclick={() => links.linkAction(action.id, linkInfo)}>
                            {action.label}
                        </button>
                    {/each}
                {:else if linkKind === 'external'}
                    <button
                        class="pop primary"
                        onmousedown={keepPopoverFocus}
                        onclick={() => links.openExternalLink(linkInfo.href)}>
                        {ENTRY_EDITOR_TEXT.openExternalLink}
                    </button>
                {/if}
                {#if links.linkNote}<span class="pop-note">{links.linkNote}</span>{/if}
            </div>
        {/if}
        {@render picker?.()}
        {#if saveConflict}
            <p class="note conflict" role="alert">
                {ENTRY_EDITOR_TEXT.conflictMessage}
            </p>
        {/if}
        {#if links.navError}
            <p class="note nav-error" role="alert">
                {links.navError}
                <button class="mini" onclick={links.retryPendingNav} disabled={links.navigating}>
                    {ENTRY_EDITOR_TEXT.retry}
                </button>
            </p>
        {/if}
        {#if deleteError}
            <p class="note nav-error" role="alert">
                {deleteError}
            </p>
        {/if}
        <EntryFooter
            {status}
            {saveConflict}
            {history}
            navigating={links.navigating}
            goBack={onBack}
            remove={deleteEntry} />
    </div>
{:else if loadFailed}
    <p class="note" role="status">
        {loadProblemText(loadProblem)}
    </p>
    <Button kind="quiet-button" handler={onBackFromLoadError} aria-label={ENTRY_EDITOR_TEXT.home}
        ><IconLeft />{ENTRY_EDITOR_TEXT.home}</Button>
{:else}
    <p class="note">{ENTRY_EDITOR_TEXT.opening}</p>
{/if}

<style src="./styles.css"></style>
