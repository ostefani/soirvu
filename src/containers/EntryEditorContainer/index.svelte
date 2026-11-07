<script>
    import { onDestroy, onMount } from 'svelte';
    import * as api from '@/api.js';
    import { registerFlushHook } from '@/lib/lock-hooks.js';
    import { notify } from '@/components/Notification/store.svelte.js';
    import { EVENT_TYPE } from '@/constants.js';
    import EntryEditor from '@/components/EntryEditor/index.svelte';
    import LinkPanelsContainer from '@/containers/LinkPanelsContainer/index.svelte';
    import LinkPickerContainer from '@/containers/LinkPickerContainer/index.svelte';
    import { createEntrySession } from './session.svelte.js';

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

    const session = createEntrySession({
        api,
        onSaved: (...args) => onSaved?.(...args),
        onDeleted: (...args) => onDeleted?.(...args),
        onOpenEntry: (target) => onOpenEntry?.(target),
        notifyError: (message) => notify(EVENT_TYPE.ERROR, message),
        notifyCopied: (message) => notify(EVENT_TYPE.COPIED, message),
    });

    onDestroy(() => session.dispose());
    onMount(() => session.load(entryId));
    onMount(() => registerFlushHook(() => session.prepareLock()));

    /** Diary-facing gate: settle the open entry before switching. */
    export function flushForNavigation() {
        return session.prepareNavigation();
    }

    /** The current selection, for history restores. */
    export function navCapture() {
        return session.navCapture();
    }

    export function openFolderId() {
        return session.entry ? (session.entry.folder_id ?? null) : undefined;
    }
</script>

{#snippet panels(open)}
    {#if session.entry}
        <LinkPanelsContainer
            entryId={session.entry.id}
            showBack={open}
            {folders}
            {workspaceTick}
            onOpenEntry={(id) => session.followEntryLink(id)} />
    {/if}
{/snippet}

{#snippet picker()}
    {#if session.linkPicker}
        <div class="picker-overlay">
            <button
                type="button"
                class="picker-backdrop"
                tabindex="-1"
                aria-label="Close link picker"
                onclick={() => session.cancelPicker()}></button>
            <LinkPickerContainer
                initialQuery={session.linkPicker.initialText}
                initialLabel={session.linkPicker.label ?? ''}
                canRemove={session.linkPicker.canRemove ?? false}
                onRemoveLink={() => session.removePickerLink()}
                {folders}
                onPick={(target) => session.pickLinkTarget(target)}
                onCancel={() => session.cancelPicker()} />
        </div>
    {/if}
{/snippet}

<EntryEditor
    onViewApi={session.setViewApi}
    entry={session.entry}
    loadFailed={session.loadFailed}
    loadProblem={session.loadProblem}
    saveConflict={session.saveConflict}
    frozen={session.frozen}
    status={session.status}
    title={session.title}
    entryDate={session.entryDate}
    mood={session.mood}
    tags={session.tags}
    initialBody={session.bodyMd}
    navError={session.navError}
    navigating={session.navigating}
    linkNote={session.linkNote}
    deleteError={session.deleteError}
    copying={session.copying}
    onCopyNoteLink={() => session.copyNoteLink()}
    pickerOpen={session.linkPicker !== null}
    {folders}
    {navRestore}
    {canGoBack}
    {canGoForward}
    {onBack}
    {onNavBack}
    {onNavForward}
    {panels}
    {picker}
    onField={(name, value) => session.setField(name, value)}
    onBodyChange={(markdown) => session.setBody(markdown)}
    attachFiles={(files) => session.attachFiles(files)}
    settleForNavigation={() => session.prepareNavigation()}
    closeSaves={() => session.closeSaves()}
    deleteEntry={() => session.deleteEntry()}
    fetchAttachmentMeta={(id) => session.fetchAttachmentMeta(id)}
    attachmentUrlFor={(id) => session.attachmentUrlFor(id)}
    openAttachment={(id) => session.openAttachment(id)}
    openExternalLink={(href) => session.openExternalLink(href)}
    openHref={(href, modified) => session.openHref(href, modified)}
    onLinkEntry={() => session.openLinkPicker()}
    onLinkAction={(id, link) => session.linkAction(id, link)}
    onRequestBracketPicker={(bracketPos) => session.requestBracketPicker(bracketPos)}
    onRetryNav={() => session.retryPendingNav()} />

<style src="./styles.css"></style>
