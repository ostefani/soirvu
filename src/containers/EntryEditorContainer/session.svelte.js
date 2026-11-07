import { createRequestOwner } from '../../lib/request-owner.js';
import { createAutosave } from '../../lib/autosave.svelte.js';
import { createEntryCommit } from '../../lib/entry-commit.js';
import { copyText as defaultCopyText } from '../../lib/clipboard.js';
import { AUTOSAVE_DELAY } from './constants.js';
import { SESSION_TEXT, SAVE_LABELS } from './data.js';
import { createDraft } from './draft.svelte.js';
import { createLinks } from './links.svelte.js';
import { createUploads } from './uploads.svelte.js';

export function createEntrySession({
    api,
    delay = AUTOSAVE_DELAY,
    onSaved = null,
    onDeleted = null,
    onOpenEntry = null,
    notifyError = null,
    notifyCopied = null,
    copyText = defaultCopyText,
}) {
    let entry = $state.raw(null);
    let loadFailed = $state(false);
    let loadProblem = $state(null);
    let saveConflict = $state(false);
    let frozen = $state(false);
    let deleteError = $state(null);
    let engine = null;
    let commitSeq = null;
    let started = false;

    const loadOwner = createRequestOwner();
    const draft = createDraft({ onEdit: () => autosave.touch() });

    const autosave = createAutosave({
        snapshot: () => (entry ? draft.snapshot(entry.tz) : null),
        send: async (payload) => {
            try {
                entry = await commitSeq.commit((base) => api.entryUpdate(entry.id, base, payload));
            } catch (e) {
                if (e === 'revision_conflict') saveConflict = true;
                throw e;
            }
            onSaved?.();
        },
        delay,
    });

    const uploads = createUploads({
        api,
        autosave,
        canUpload: () => entry !== null && engine !== null && !frozen,
        entryId: () => entry.id,
        commit: (run) => commitSeq.commit(run),
        onAdded({ id, mime, name, rev }) {
            if (entry) entry = { ...entry, rev };
            engine?.insertAttachment({ id, mime, name });
        },
        onError(error) {
            if (error === 'revision_conflict') saveConflict = true;
        },
    });

    const links = createLinks({
        api,
        getEntry: () => entry,
        getEditor: () => engine,
        isFrozen: () => frozen,
        draft,
        onOpenEntry,
        copyText,
        notifyError,
        notifyCopied,
    });

    function setField(name, value) {
        if (name === 'tags') draft.tags = value;
        else if (name === 'title' || name === 'entryDate' || name === 'mood') draft[name] = value;
    }

    function setBody(markdown) {
        draft.body = markdown;
    }

    async function load(id) {
        if (started) return;
        started = true;
        const token = loadOwner.begin();
        try {
            const loaded = await api.entryGet(id);
            if (!loadOwner.current(token)) return;
            draft.load(loaded);
            commitSeq = createEntryCommit(loaded.rev);
            entry = loaded;
        } catch (e) {
            if (!loadOwner.current(token)) return;
            loadFailed = true;
            if (e === 'entry_not_found') loadProblem = 'deleted';
            else if (e === 'invalid_input') loadProblem = 'missing';
            else loadProblem = 'error';
        }
    }

    function dispose() {
        loadOwner.dispose();
        links.dispose();
        autosave.cancel();
    }

    function attachEditor(instance) {
        engine = instance;
        return () => {
            closeSaves();
            engine = null;
        };
    }

    async function settle() {
        await uploads.settle();
        await autosave.flush();
        return autosave.clean && uploads.pending === 0;
    }

    async function prepareNavigation() {
        if (!entry || !engine || frozen) return true;
        engine.finishComposing();
        return settle();
    }

    async function prepareLock() {
        frozen = true;
        links.invalidateRefresh();
        await uploads.abort();
        await autosave.flush();
        const ok = autosave.clean && uploads.pending === 0;
        if (!ok) {
            frozen = false;
            uploads.resume();
            notifyError?.(SESSION_TEXT.latestEditsLost);
        }
        return ok;
    }

    function navCapture() {
        const selection = engine?.query();
        return selection ? { from: selection.from, to: selection.to } : null;
    }

    async function flushSaves() {
        await autosave.flush();
        return autosave.clean;
    }

    function closeSaves() {
        autosave.close().then((ok) => {
            if (!ok) notifyError?.(SESSION_TEXT.latestEditsLost);
        });
    }

    async function deleteEntry() {
        if (frozen || !entry) return false;
        deleteError = null;
        if (!(await settle())) {
            deleteError = SESSION_TEXT.deleteSaveFailed;
            return false;
        }
        autosave.discard();
        try {
            await api.entryDelete(entry.id);
        } catch {
            deleteError = SESSION_TEXT.deleteFailed;
            return false;
        }
        onDeleted?.();
        return true;
    }

    function fetchAttachmentMeta(id) {
        return api.attachmentInfo(id);
    }

    function attachmentUrlFor(id) {
        return api.assetUrl(id);
    }

    function openAttachment(id) {
        api.attachmentOpen(id).catch(() => { });
    }

    return {
        draft,
        links,
        get entry() {
            return entry;
        },
        get loadFailed() {
            return loadFailed;
        },
        get loadProblem() {
            return loadProblem;
        },
        get saveConflict() {
            return saveConflict;
        },
        get frozen() {
            return frozen;
        },
        get status() {
            return uploads.status || SAVE_LABELS[autosave.state];
        },
        get dirty() {
            return !autosave.clean;
        },
        get deleteError() {
            return deleteError;
        },
        load,
        dispose,
        attachEditor,
        setField,
        setBody,
        attachFiles: uploads.attach,
        prepareNavigation,
        prepareLock,
        navCapture,
        flushSaves,
        deleteEntry,
        fetchAttachmentMeta,
        attachmentUrlFor,
        openAttachment,
    };
}
