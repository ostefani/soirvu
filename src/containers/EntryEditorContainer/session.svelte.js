// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
import { createRequestOwner } from '../../lib/request-owner.js';
import { createAutosave } from '../../lib/autosave.svelte.js';
import { createEntryCommit, mayFinishAfterFlush } from '../../lib/entry-commit.js';
import { classifyHref, parseEntryTarget } from '../../lib/entry-target.js';
import { ATTACH_CHUNK_SIZE, AUTOSAVE_DELAY, MAX_CONCURRENT_UPLOADS } from './constants.js';
import {
    ENTRY_EDITOR_TEXT,
    SAVE_LABELS,
    attachError,
    attachmentStatus,
    defaultLinkLabel,
    linkTargetError,
    uploadFailureText,
} from './data.js';

export function parseTags(text) {
    return text
        .split(',')
        .map((t) => t.trim())
        .filter((t) => t !== '');
}

async function defaultCopyText(text) {
    try {
        await navigator.clipboard.writeText(text);
        return;
    } catch {
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
}

export function createEntrySession({
    api,
    delay = AUTOSAVE_DELAY,
    onSaved = null,
    onDeleted = null,
    onOpenEntry = null,
    notifyError = null,
    copyText = defaultCopyText,
}) {
    let entry = $state(null);
    let loadFailed = $state(false);
    let loadProblem = $state(null);
    let saveConflict = $state(false);
    let frozen = $state(false);
    let title = $state('');
    let entryDate = $state('');
    let mood = $state('');
    let tagsText = $state('');
    let bodyMd = $state('');
    let pendingAttachments = $state(0);
    let attachmentFailures = $state.raw([]);
    let linkPicker = $state(null);
    let navigating = $state(false);
    let navError = $state('');
    let pendingNavId = $state(null);
    let deleteError = $state(null);
    let linkNote = $state('');

    let viewApi = null;
    let commitSeq = null;
    let started = false;

    const loadOwner = createRequestOwner();
    const attachmentQueue = [];
    let activeAttachments = 0;
    let abortUploads = false;
    const inflightUploads = new Set();

    const autosave = createAutosave({
        snapshot: () => snapshot(),
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

    function snapshot() {
        if (!entry) return null;
        return {
            entry_date: entryDate,
            title: title.trim() === '' ? null : title.trim(),
            body_md: bodyMd,
            mood: mood === '' ? null : Number(mood),
            tz: entry.tz,
            tags: parseTags(tagsText),
        };
    }

    function statusText() {
        return attachmentStatus(pendingAttachments, attachmentFailures) || SAVE_LABELS[autosave.state];
    }

    async function load(id) {
        if (started) return;
        started = true;
        const token = loadOwner.begin();
        try {
            const loaded = await api.entryGet(id);
            if (!loadOwner.current(token)) return;
            title = loaded.title ?? '';
            entryDate = loaded.entry_date;
            mood = loaded.mood == null ? '' : String(loaded.mood);
            tagsText = loaded.tags.join(', ');
            bodyMd = loaded.body_md;
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
        autosave.cancel();
    }

    function setViewApi(next) {
        viewApi = next;
    }

    function setField(name, value) {
        if (name === 'title') title = value;
        else if (name === 'entryDate') entryDate = value;
        else if (name === 'mood') mood = value;
        else if (name === 'tagsText') tagsText = value;
        else return;
        autosave.touch();
    }

    function setBody(markdown) {
        bodyMd = markdown;
        autosave.touch();
    }

    function attachFiles(files) {
        if (!entry || !viewApi || frozen) return;
        if (pendingAttachments === 0) attachmentFailures = [];
        for (const file of files) attachmentQueue.push(file);
        pendingAttachments += files.length;
        drainQueue();
    }

    function drainQueue() {
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
                    drainQueue();
                });
            inflightUploads.add(done);
        }
    }

    async function uploadFile(file) {
        let uploadId = null;
        try {
            uploadId = await api.attachmentBegin(entry.id, file.name, file.type || null);
            for (let offset = 0; offset < file.size; offset += ATTACH_CHUNK_SIZE) {
                if (abortUploads) {
                    await api.attachmentAbort(uploadId).catch(() => {});
                    return null;
                }
                const slice = await file.slice(offset, offset + ATTACH_CHUNK_SIZE).arrayBuffer();
                await api.attachmentChunk(uploadId, new Uint8Array(slice));
            }
            const saved = await autosave.flush();
            if (!mayFinishAfterFlush(saved, autosave.state)) {
                await api.attachmentAbort(uploadId).catch(() => {});
                uploadId = null;
                return attachError(file.name, ENTRY_EDITOR_TEXT.staleAttach);
            }
            const added = await commitSeq.commit((base) => api.attachmentFinish(uploadId, base));
            uploadId = null;
            if (entry) entry = { ...entry, rev: added.rev };
            viewApi?.insertAttachment?.({ id: added.id, mime: added.mime, name: file.name });
            return null;
        } catch (e) {
            if (uploadId) await api.attachmentAbort(uploadId).catch(() => {});
            if (e === 'revision_conflict') saveConflict = true;
            return uploadFailureText(file.name, e);
        }
    }

    async function abortAllUploads() {
        abortUploads = true;
        const queued = attachmentQueue.length;
        attachmentQueue.length = 0;
        pendingAttachments -= queued;
        await Promise.allSettled([...inflightUploads]);
    }

    async function settle() {
        while (!abortUploads && (attachmentQueue.length > 0 || inflightUploads.size > 0)) {
            await Promise.allSettled([...inflightUploads]);
        }
        await autosave.flush();
        return autosave.clean && pendingAttachments === 0;
    }

    async function prepareNavigation() {
        if (!entry || !viewApi || frozen) return true;
        viewApi.finishComposing();
        return settle();
    }

    async function prepareLock() {
        frozen = true;
        await abortAllUploads();
        await autosave.flush();
        const ok = autosave.clean && pendingAttachments === 0;
        if (!ok) {
            frozen = false;
            abortUploads = false;
            notifyError?.(ENTRY_EDITOR_TEXT.latestEditsLost);
        }
        return ok;
    }

    function navCapture() {
        return viewApi?.navCapture?.() ?? null;
    }

    async function flushSaves() {
        await autosave.flush();
        return autosave.clean;
    }

    function closeSaves() {
        autosave.close().then((ok) => {
            if (!ok) notifyError?.(ENTRY_EDITOR_TEXT.latestEditsLost);
        });
    }

    async function deleteEntry() {
        if (frozen || !entry) return false;
        deleteError = null;
        if (!(await settle())) {
            deleteError = ENTRY_EDITOR_TEXT.deleteSaveFailed;
            return false;
        }
        autosave.discard();
        try {
            await api.entryDelete(entry.id);
        } catch {
            deleteError = ENTRY_EDITOR_TEXT.deleteFailed;
            return false;
        }
        onDeleted?.();
        return true;
    }

    function openLinkPicker() {
        if (frozen || !viewApi) return;
        linkPicker = { bracketPos: null, initialText: '' };
    }

    function requestBracketPicker(bracketPos) {
        if (frozen) return;
        linkPicker = { bracketPos, initialText: '' };
    }

    function cancelPicker() {
        linkPicker = null;
        viewApi?.focus();
    }

    function pickLinkTarget(target) {
        const picker = linkPicker;
        if (!viewApi || !picker) return;
        linkPicker = null;
        let from;
        let to;
        let label;
        if (picker.bracketPos != null) {
            from = picker.bracketPos;
            to = picker.bracketPos + 2;
            label = defaultLinkLabel(target);
        } else {
            const sel = viewApi.query();
            if (!sel) {
                viewApi.focus();
                return;
            }
            from = sel.from;
            to = sel.to;
            label = sel.empty ? defaultLinkLabel(target) : viewApi.readText(from, to);
        }
        if (!label.trim()) {
            viewApi.focus();
            return;
        }
        viewApi.exec('insertEntryLink', { targetId: target.id, label, from, to });
        viewApi.focus();
    }

    async function followEntryLink(id) {
        if (navigating || frozen || !entry) return;
        navigating = true;
        navError = '';
        try {
            if (!(await prepareNavigation())) {
                pendingNavId = id;
                navError = ENTRY_EDITOR_TEXT.navigateAfterSave;
                return;
            }
            pendingNavId = null;
            const sel = viewApi?.query();
            onOpenEntry?.({ id, selFrom: sel?.from ?? 0, selTo: sel?.to ?? 0 });
        } finally {
            navigating = false;
        }
    }

    async function retryPendingNav() {
        if (pendingNavId == null) return;
        const id = pendingNavId;
        pendingNavId = null;
        await followEntryLink(id);
    }

    function openHref(href, modified) {
        if (!modified || frozen) return;
        const kind = classifyHref(href);
        if (kind === 'entry') {
            const id = parseEntryTarget(href);
            if (id) followEntryLink(id);
        } else if (kind === 'external') {
            openExternalLink(href);
        }
    }

    function liveLinkRange(link) {
        const live = viewApi?.locateLink(link.href, link.from) ?? null;
        if (!live) linkNote = ENTRY_EDITOR_TEXT.linkMoved;
        return live;
    }

    function linkAction(id, link) {
        if (id === 'open') {
            const targetId = parseEntryTarget(link.href);
            if (targetId) followEntryLink(targetId);
        } else if (id === 'copy') copyInternalLink(link);
        else if (id === 'refresh') refreshLinkLabel(link);
        else if (id === 'remove') removeLinkMark(link);
    }

    function removeLinkMark(link) {
        const live = liveLinkRange(link);
        if (live) viewApi.exec('removeLink', live);
    }

    function removeLinkRange(range) {
        viewApi?.exec('removeLink', range);
    }

    async function copyInternalLink(link) {
        const live = liveLinkRange(link);
        if (!live) return;
        const label = viewApi.readText(live.from, live.to);
        try {
            await copyText(`[${label}](${link.href})`);
            linkNote = (await noteLinkCopied())
                ? ENTRY_EDITOR_TEXT.linkCopied
                : ENTRY_EDITOR_TEXT.linkCopyFailed;
        } catch {
            linkNote = ENTRY_EDITOR_TEXT.linkCopyFailed;
        }
    }

    async function refreshLinkLabel(link) {
        const id = parseEntryTarget(link.href);
        if (!id || !viewApi) return;
        try {
            const target = await fetchLinkTarget(id);
            const live = liveLinkRange(link);
            if (!live) return;
            viewApi.exec('insertEntryLink', {
                targetId: id,
                label: defaultLinkLabel(target),
                from: live.from,
                to: live.to,
            });
            linkNote = '';
        } catch (e) {
            linkNote =
                e === 'entry_not_found' ? ENTRY_EDITOR_TEXT.targetDeleted : linkTargetError(e);
        }
    }

    function readBody() {
        return viewApi?.getMarkdown() ?? entry?.body_md ?? '';
    }

    function fetchLinkTarget(id) {
        return api.entryGet(id);
    }

    async function noteLinkCopied() {
        try {
            await api.clipboardNoteCopy();
            return true;
        } catch {
            return false;
        }
    }

    function fetchAttachmentMeta(id) {
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

    return {
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
        get title() {
            return title;
        },
        get entryDate() {
            return entryDate;
        },
        get mood() {
            return mood;
        },
        get tags() {
            return parseTags(tagsText);
        },
        get bodyMd() {
            return bodyMd;
        },
        get status() {
            return statusText();
        },
        get dirty() {
            return !autosave.clean;
        },
        get linkPicker() {
            return linkPicker;
        },
        get navigating() {
            return navigating;
        },
        get navError() {
            return navError;
        },
        get pendingNavId() {
            return pendingNavId;
        },
        get deleteError() {
            return deleteError;
        },
        get linkNote() {
            return linkNote;
        },
        load,
        dispose,
        setViewApi,
        setField,
        setBody,
        attachFiles,
        prepareNavigation,
        prepareLock,
        navCapture,
        flushSaves,
        closeSaves,
        deleteEntry,
        openLinkPicker,
        requestBracketPicker,
        cancelPicker,
        pickLinkTarget,
        followEntryLink,
        retryPendingNav,
        openHref,
        linkAction,
        readBody,
        removeLinkRange,
        fetchAttachmentMeta,
        attachmentUrlFor,
        openAttachment,
        openExternalLink,
    };
}
