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

    const { draft, links } = session;
    const history = {
        get canGoBack() {
            return canGoBack;
        },
        get canGoForward() {
            return canGoForward;
        },
        back: () => onNavBack?.(),
        forward: () => onNavForward?.(),
    };
    const attachments = {
        info: session.fetchAttachmentMeta,
        url: session.attachmentUrlFor,
        open: session.openAttachment,
    };

    async function leave() {
        if (!session.frozen && (await session.prepareNavigation())) onBack?.();
    }

    onDestroy(() => session.dispose());
    onMount(() => {
        session.load(entryId);
    });
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
            onOpenEntry={(id) => links.followEntryLink(id)} />
    {/if}
{/snippet}

{#snippet picker()}
    {#if links.linkPicker}
        <div class="picker-overlay">
            <button
                type="button"
                class="picker-backdrop"
                tabindex="-1"
                aria-label="Close link picker"
                onclick={() => links.cancelPicker()}></button>
            <LinkPickerContainer
                initialQuery={links.linkPicker.initialText}
                initialLabel={links.linkPicker.label ?? ''}
                canRemove={links.linkPicker.canRemove ?? false}
                onRemoveLink={() => links.removePickerLink()}
                {folders}
                onPick={(target) => links.pickLinkTarget(target)}
                onCancel={() => links.cancelPicker()} />
        </div>
    {/if}
{/snippet}

<EntryEditor
    ready={session.entry !== null}
    loadFailed={session.loadFailed}
    loadProblem={session.loadProblem}
    saveConflict={session.saveConflict}
    frozen={session.frozen}
    status={session.status}
    deleteError={session.deleteError}
    {draft}
    {links}
    {attachments}
    {history}
    {navRestore}
    {panels}
    {picker}
    onBack={leave}
    onBackFromLoadError={() => onBack?.()}
    onEditorReady={session.attachEditor}
    onField={session.setField}
    onBodyChange={session.setBody}
    attachFiles={session.attachFiles}
    deleteEntry={session.deleteEntry} />

<style src="./styles.css"></style>
