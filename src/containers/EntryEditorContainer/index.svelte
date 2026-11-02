<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy, onMount } from 'svelte';
    import * as api from '@/api.js';
    import { createRequestOwner } from '@/lib/request-owner.js';
    import { createAutosave } from '@/lib/autosave.svelte.js';
    import { createEntryCommit, mayFinishAfterFlush } from '@/lib/entry-commit.js';
    import { registerFlushHook } from '@/lib/lock-hooks.js';
    import { notify } from '@/components/Notification/store.svelte.js';
    import { EVENT_TYPE } from '@/constants.js';
    import EntryEditor from '@/components/EntryEditor/index.svelte';

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
    /* Sticky once set: the open rev is behind the stored one, so
       every later save with it fails too. Local edits are kept;
       reopening the entry remounts with the current rev. */
    let saveConflict = $state(false);
    let frozen = $state(false);

    // The view's narrow engine interface (getSnapshot,
    // insertAttachment, finishComposing, navCapture). The engine
    // instance itself never leaves the view.
    let viewApi = $state.raw(null);

    /* One entry's commit sequence: content saves and attachment
       completions run exclusively, in submission order, each with
       the newest committed base. */
    let commitSeq = null;

    // The initial entry load: disposal (navigation, lock) retires
    // it, so a late response cannot publish into a torn-down pane.
    const loadOwner = createRequestOwner();

    onDestroy(() => loadOwner.dispose());

    const autosave = createAutosave({
        snapshot: () => viewApi?.getSnapshot?.() ?? null,
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
        }
    });

    onMount(() =>
        registerFlushHook(async () => {
            frozen = true;
            await abortAllUploads();
            await autosave.flush();
            const ok = autosave.clean && pendingAttachments === 0;
            if (!ok) {
                frozen = false;
                abortUploads = false;
                notify(EVENT_TYPE.ERROR, 'Latest edits could not be saved.');
            }
            return ok;
        }),
    );

    const ATTACH_CHUNK = 1024 * 1024;

    async function abortAllUploads() {
        abortUploads = true;
        const queued = attachmentQueue.length;
        attachmentQueue.length = 0;
        pendingAttachments -= queued;
        await Promise.allSettled([...inflightUploads]);
    }

    function attachFiles(files) {
        if (!entry || !viewApi || frozen) return;
        if (pendingAttachments === 0) attachmentFailures = [];
        for (const file of files) attachmentQueue.push(file);
        pendingAttachments += files.length;
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
            viewApi?.insertAttachment?.({ id: added.id, mime: added.mime, name: file.name });
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

    /* Settles uploads and flushes for container-gated switches
       (link follow, back, list, search results, history). False
       stays put: the foot already shows the save state. */
    async function settleForNavigation() {
        await Promise.allSettled([...inflightUploads]);
        await autosave.flush();
        return autosave.clean && pendingAttachments === 0;
    }

    /** Diary-facing gate: settle the open entry before switching. */
    export async function flushForNavigation() {
        if (!entry || !viewApi || frozen) return true;
        viewApi.finishComposing();
        return settleForNavigation();
    }

    /** The current selection, for history restores. */
    export function navCapture() {
        return viewApi?.navCapture?.() ?? null;
    }

    /** Marks the draft dirty (view reports engine/input changes). */
    function touch() {
        autosave.touch();
    }

    /** Flushes saves; the view stays on the entry when false. */
    async function flushSaves() {
        await autosave.flush();
        return autosave.clean;
    }

    /** Final save at teardown with the engine still alive; the view
        calls this before destroying its editor so the last edits
        are not skipped. */
    function closeSaves() {
        autosave.close().then((ok) => {
            if (!ok) notify(EVENT_TYPE.ERROR, 'Latest edits could not be saved.');
        });
    }

    /* Save first; the view stays on the entry when deletion cannot run. */
    async function deleteEntry() {
        if (frozen || !entry) return;
        autosave.discard(); // never save an entry that is being deleted
        await api.entryDelete(entry.id);
        onDeleted?.();
    }

    /** Link-target metadata for the view's label refresh. */
    async function fetchLinkTarget(id) {
        return api.entryGet(id);
    }

    /** Clipboard bookkeeping after the view copied link text. */
    async function noteLinkCopied() {
        try {
            await api.clipboardNoteCopy();
            return true;
        } catch {
            return false;
        }
    }

    /** Attachment preview metadata for the view's hydration. */
    async function fetchAttachmentMeta(id) {
        return api.attachmentInfo(id);
    }

    function attachmentUrlFor(id) {
        return api.assetUrl(id);
    }

    function openAttachment(id) {
        api.attachmentOpen(id).catch(() => {});
    }

    function openExternalLink(href) {
        api.openLink(href).catch(() => {});
    }
</script>

<EntryEditor
    bind:this={viewApi}
    {entry}
    {loadFailed}
    {loadProblem}
    {saveConflict}
    {frozen}
    {status}
    {folders}
    {workspaceTick}
    {navRestore}
    {canGoBack}
    {canGoForward}
    {onBack}
    {onOpenEntry}
    {onNavBack}
    {onNavForward}
    {touch}
    {attachFiles}
    {settleForNavigation}
    {flushSaves}
    {closeSaves}
    {deleteEntry}
    {fetchLinkTarget}
    {noteLinkCopied}
    {fetchAttachmentMeta}
    {attachmentUrlFor}
    {openAttachment}
    {openExternalLink} />
