import { createRequestOwner } from '../../lib/request-owner.js';
import { entryLinkMarkdown } from '../../lib/links.js';
import { classifyHref, parseEntryTarget } from '../../lib/entry-target.js';
import { SESSION_TEXT } from './data.js';
import { defaultLinkLabel, linkTargetError } from './labels.js';

export function createLinks({ api, getEntry, getEditor, isFrozen, draft, onOpenEntry, copyText, notifyError, notifyCopied }) {
    let linkPicker = $state(null);
    let navigating = $state(false);
    let navError = $state('');
    let pendingNavId = $state(null);
    let linkNote = $state('');
    let copying = $state(false);
    const copyOwner = createRequestOwner();
    const refreshOwner = createRequestOwner();

    function openLinkPicker() {
        const editor = getEditor();
        if (isFrozen() || !editor) return;
        const selection = editor.query();
        if (!selection) return;
        const from = selection.link?.from ?? selection.from;
        const to = selection.link?.to ?? selection.to;
        linkPicker = { bracketPos: null, initialText: '', from, to, label: editor.readText(from, to), canRemove: !!(selection.link || selection.hasLink) };
    }

    function requestBracketPicker(bracketPos) {
        if (isFrozen()) return;
        linkPicker = { bracketPos, initialText: '' };
    }

    function cancelPicker() {
        linkPicker = null;
        getEditor()?.focus();
    }

    function removePickerLink() {
        const editor = getEditor();
        if (isFrozen() || !editor || !linkPicker?.canRemove) return;
        const { from, to } = linkPicker;
        editor.exec('removeLink', { from, to });
        cancelPicker();
    }

    function pickLinkTarget(target) {
        const editor = getEditor();
        const picker = linkPicker;
        if (!editor || !picker || isFrozen()) return;
        linkPicker = null;
        let from;
        let to;
        let label;
        if (picker.bracketPos != null) {
            from = picker.bracketPos;
            to = picker.bracketPos + 2;
            label = target.linkLabel?.trim() || defaultLinkLabel(target);
        } else {
            const sel = picker.from != null ? picker : editor.query();
            if (!sel) {
                editor.focus();
                return;
            }
            from = sel.from;
            to = sel.to;
            label = target.linkLabel?.trim() || (from === to ? defaultLinkLabel(target) : editor.readText(from, to));
        }
        if (!label.trim()) {
            editor.focus();
            return;
        }
        editor.exec('insertEntryLink', { targetId: target.id, label, from, to });
        editor.focus();
    }

    async function followEntryLink(id) {
        if (navigating || isFrozen() || !getEntry()) return;
        navigating = true;
        navError = '';
        try {
            const sel = getEditor()?.query();
            const ok = await onOpenEntry?.({ id, selFrom: sel?.from ?? 0, selTo: sel?.to ?? 0 });
            if (ok === false) {
                pendingNavId = id;
                navError = SESSION_TEXT.navigateAfterSave;
                return;
            }
            pendingNavId = null;
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
        if (!modified || isFrozen()) return;
        const kind = classifyHref(href);
        if (kind === 'entry') {
            const id = parseEntryTarget(href);
            if (id) followEntryLink(id);
        } else if (kind === 'external') {
            openExternalLink(href);
        }
    }

    function liveLinkRange(link) {
        const live = getEditor()?.locateLink(link.href, link.from) ?? null;
        if (!live) linkNote = SESSION_TEXT.linkMoved;
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
        if (live) getEditor().exec('removeLink', live);
    }

    async function copyLinkText(text) {
        if (isFrozen() || copying) return false;
        const token = copyOwner.begin();
        copying = true;
        try {
            await copyText(text);
            const registered = await noteLinkCopied();
            if (!copyOwner.current(token) || isFrozen()) return false;
            if (registered) notifyCopied?.(SESSION_TEXT.linkCopied);
            else notifyError?.(SESSION_TEXT.linkCopyFailed);
            return registered;
        } catch {
            if (copyOwner.current(token) && !isFrozen()) notifyError?.(SESSION_TEXT.linkCopyFailed);
            return false;
        } finally {
            if (copyOwner.current(token)) copying = false;
        }
    }

    async function copyNoteLink() {
        if (!getEntry() || isFrozen()) return false;
        const label = defaultLinkLabel({ title: draft.title.trim(), entry_date: draft.entryDate });
        return copyLinkText(entryLinkMarkdown(getEntry().id, label));
    }

    async function copyInternalLink(link) {
        if (isFrozen()) return false;
        const live = liveLinkRange(link);
        if (!live) return false;
        const label = getEditor().readText(live.from, live.to);
        return copyLinkText(entryLinkMarkdown(parseEntryTarget(link.href), label));
    }

    async function refreshLinkLabel(link) {
        const id = parseEntryTarget(link.href);
        const editor = getEditor();
        if (!id || !editor || isFrozen()) return;
        const token = refreshOwner.begin();
        const isCurrent = () => refreshOwner.current(token) && !isFrozen() && getEditor() === editor;
        try {
            const target = await api.entryGet(id);
            if (!isCurrent()) return;
            const live = liveLinkRange(link);
            if (!live) return;
            editor.exec('insertEntryLink', {
                targetId: id,
                label: defaultLinkLabel(target),
                from: live.from,
                to: live.to,
            });
            linkNote = '';
        } catch (e) {
            if (!isCurrent()) return;
            linkNote =
                e === 'entry_not_found' ? SESSION_TEXT.targetDeleted : linkTargetError(e);
        }
    }

    async function noteLinkCopied() {
        try {
            await api.clipboardNoteCopy();
            return true;
        } catch {
            return false;
        }
    }

    function openExternalLink(href) {
        api.openLink(href).catch(() => { });
    }

    return {
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
        get linkNote() {
            return linkNote;
        },
        get copying() {
            return copying;
        },
        invalidateRefresh: refreshOwner.invalidate,
        dispose() {
            copyOwner.dispose();
            refreshOwner.dispose();
        },
        copyNoteLink,
        openLinkPicker,
        requestBracketPicker,
        cancelPicker,
        removePickerLink,
        pickLinkTarget,
        followEntryLink,
        retryPendingNav,
        openHref,
        linkAction,
        openExternalLink,
    };
}
