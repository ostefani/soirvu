import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseTags } from '../../../lib/tags.js';

const stateHook = (value) => value;
stateHook.raw = (value) => value;
globalThis.$state = stateHook;

const { createEntrySession } = await import('../session.svelte.js');

function deferred() {
    let resolve;
    let reject;
    const promise = new Promise((res, rej) => {
        resolve = res;
        reject = rej;
    });
    return { promise, resolve, reject };
}

function fakeFile(name = 'a.png', size = 3) {
    return {
        name,
        type: 'image/png',
        size,
        slice: (from, to) => ({
            arrayBuffer: async () => new ArrayBuffer(Math.max(0, to - from)),
        }),
    };
}

function fakeViewApi(overrides = {}) {
    const calls = [];
    return {
        calls,
        finishComposing() {},
        getMarkdown() {
            return 'live body';
        },
        insertAttachment() {},
        query() {
            return { from: 0, to: 0, empty: true };
        },
        readText() {
            return '';
        },
        locateLink() {
            return { from: 0, to: 1 };
        },
        exec(name, arg) {
            calls.push([name, arg]);
            return true;
        },
        focus() {},
        ...overrides,
    };
}

function setup({ entryOverrides = {}, delay = 10000, onOpenEntry = null } = {}) {
    const saves = [];
    const api = {
        entryGet: async () => ({
            id: 'e1',
            rev: 1,
            tz: 'UTC',
            title: 'Hello',
            entry_date: '2026-10-20',
            body_md: 'stored body',
            mood: 3,
            tags: ['a'],
            ...entryOverrides,
        }),
        entryUpdate: async (id, base, payload) => {
            saves.push({ id, base, payload });
            return { rev: base + 1 };
        },
        entryDelete: async () => {},
        attachmentBegin: async () => 'u1',
        attachmentChunk: async () => {},
        attachmentFinish: async (uploadId, base) => ({ id: 'att1', mime: 'image/png', rev: base + 1 }),
        attachmentAbort: async () => {},
        attachmentInfo: async (id) => ({ id, mime: 'image/png', name: 'a.png', size: 2048 }),
        assetUrl: (id) => `asset:${id}`,
        attachmentOpen: async () => {},
        openLink: async () => {},
        clipboardNoteCopy: async () => {},
    };
    const notified = [];
    const opened = [];
    const session = createEntrySession({
        api,
        delay,
        onSaved: null,
        onDeleted: () => opened.push('deleted'),
        onOpenEntry: onOpenEntry ?? ((target) => opened.push(target)),
        notifyError: (message) => notified.push(message),
    });
    return { api, saves, notified, opened, session };
}

test('parseTags splits and cleans tag text', () => {
    assert.deepEqual(parseTags('a, b,, c '), ['a', 'b', 'c']);
});

test('load publishes the entry and initializes the draft once', async () => {
    const { session } = setup();
    await session.load('e1');
    assert.equal(session.entry.id, 'e1');
    assert.equal(session.draft.title, 'Hello');
    assert.equal(session.draft.entryDate, '2026-10-20');
    assert.equal(session.draft.mood, '3');
    assert.deepEqual(session.draft.tags, ['a']);
    session.dispose();
});

test('disposal retires a late load response', async () => {
    const gate = deferred();
    const { session } = setup();
    const api = { entryGet: () => gate.promise };
    const second = createEntrySession({ api, delay: 10000 });
    const loading = second.load('e1');
    second.dispose();
    gate.resolve({ id: 'e1', rev: 1, tz: 'UTC', title: 'Late', entry_date: '2026-10-20', body_md: '', mood: null, tags: [] });
    await loading;
    assert.equal(second.entry, null);
    assert.equal(second.loadFailed, false);
    session.dispose();
});

test('detaching the editor saves its final draft even if the session has been disposed', async () => {
    const { saves, session } = setup();
    await session.load('e1');
    const detach = session.attachEditor(fakeViewApi());
    session.setBody('Final edit');
    session.dispose();
    detach();
    await new Promise((resolve) => setImmediate(resolve));

    assert.equal(session.dirty, false);
    assert.equal(saves.length, 1);
    assert.equal(saves[0].payload.body_md, 'Final edit');
    assert.equal(session.navCapture(), null);
});

test('failed deletion keeps the draft usable and reports the error', async () => {
    const { api, saves, opened, session } = setup();
    await session.load('e1');
    session.attachEditor(fakeViewApi());
    session.setField('title', 'Edited');
    api.entryDelete = async () => {
        throw 'locked';
    };
    const ok = await session.deleteEntry();
    assert.equal(ok, false);
    assert.match(session.deleteError, /Could not delete/);
    assert.equal(opened.length, 0);
    assert.ok(saves.length >= 1);
    assert.equal(saves[saves.length - 1].payload.title, 'Edited');
    session.dispose();
});

test('deletion aborts when pending saves fail, preserving dirty state', async () => {
    const { api, session } = setup();
    await session.load('e1');
    session.attachEditor(fakeViewApi());
    session.setField('title', 'Edited');
    api.entryUpdate = async () => {
        throw 'boom';
    };
    let deleted = false;
    api.entryDelete = async () => {
        deleted = true;
    };
    const ok = await session.deleteEntry();
    assert.equal(ok, false);
    assert.equal(deleted, false);
    assert.equal(session.dirty, true);
    assert.match(session.deleteError, /Could not delete/);
    session.dispose();
});

test('edits made during a save are sent, not overwritten', async () => {
    const { api, saves, session } = setup();
    await session.load('e1');
    const gate = deferred();
    let calls = 0;
    api.entryUpdate = async (id, base, payload) => {
        calls++;
        if (calls === 1) await gate.promise;
        saves.push({ id, base, payload });
        return { rev: base + 1 };
    };
    session.setField('title', 'First');
    const flushing = session.flushSaves();
    session.setField('title', 'Second');
    gate.resolve();
    await flushing;
    await session.flushSaves();
    assert.ok(saves.length >= 2);
    assert.equal(saves[saves.length - 1].payload.title, 'Second');
    assert.equal(session.dirty, false);
    session.dispose();
});

test('navigation waits for an active upload and lands its reference', async () => {
    const { api, saves, session } = setup();
    await session.load('e1');
    const view = fakeViewApi();
    session.attachEditor(view);
    const chunkGate = deferred();
    api.attachmentChunk = () => chunkGate.promise;
    session.setField('title', 'Edited before upload');
    session.attachFiles([fakeFile()]);
    const leaving = session.prepareNavigation();
    let settled = false;
    leaving.then(() => {
        settled = true;
    });
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(settled, false);
    chunkGate.resolve();
    assert.equal(await leaving, true);
    assert.ok(saves.length >= 1);
    session.dispose();
});

test('navigation drains queued uploads beyond the first active batch', async () => {
    const { api, session } = setup();
    await session.load('e1');
    const inserted = [];
    session.attachEditor(fakeViewApi({ insertAttachment: (info) => inserted.push(info) }));
    session.setField('title', 'Edited');
    let finishes = 0;
    api.attachmentFinish = async (uploadId, base) => {
        finishes++;
        return { id: `att${finishes}`, mime: 'image/png', rev: base + 1 };
    };
    session.attachFiles(Array.from({ length: 9 }, (_, i) => fakeFile(`f${i}.png`)));
    assert.equal(await session.prepareNavigation(), true);
    assert.equal(finishes, 9);
    assert.equal(inserted.length, 9);
    assert.equal(session.dirty, false);
    session.dispose();
});

test('link follow defers to the workspace gate without preparing itself', async () => {
    const gate = [];
    const { saves, opened, session } = setup({
        onOpenEntry: async (target) => {
            gate.push(target);
            return true;
        },
    });
    await session.load('e1');
    session.attachEditor(fakeViewApi());
    session.setField('title', 'Edited');
    await session.links.followEntryLink('e2');
    assert.deepEqual(gate.map((target) => target.id), ['e2']);
    assert.deepEqual(opened, []);
    assert.equal(saves.length, 0);
    assert.equal(session.links.navError, '');
    assert.equal(session.links.pendingNavId, null);
    assert.equal(session.links.navigating, false);
    session.dispose();
});

test('failed workspace navigation preserves the source with a retryable error', async () => {
    let allow = false;
    const { saves, session } = setup({ onOpenEntry: async () => allow });
    await session.load('e1');
    session.attachEditor(fakeViewApi());
    session.setField('title', 'Edited');
    await session.links.followEntryLink('e2');
    assert.match(session.links.navError, /Retry to navigate/);
    assert.equal(session.links.pendingNavId, 'e2');
    assert.equal(session.entry.id, 'e1');
    assert.equal(session.links.navigating, false);
    assert.equal(saves.length, 0);
    allow = true;
    await session.links.retryPendingNav();
    assert.equal(session.links.navError, '');
    assert.equal(session.links.pendingNavId, null);
    session.dispose();
});

test('link follow stays put while frozen', async () => {
    let calls = 0;
    const { session } = setup({ onOpenEntry: async () => (calls++, true) });
    await session.load('e1');
    session.attachEditor(fakeViewApi());
    assert.equal(await session.prepareLock(), true);
    await session.links.followEntryLink('e2');
    assert.equal(calls, 0);
    assert.equal(session.links.navError, '');
    session.dispose();
});

test('failed lock preparation unfreezes and notifies', async () => {
    const { api, notified, session } = setup();
    await session.load('e1');
    session.setField('title', 'Edited');
    api.entryUpdate = async () => {
        throw 'boom';
    };
    const ok = await session.prepareLock();
    assert.equal(ok, false);
    assert.equal(session.frozen, false);
    assert.equal(notified.length, 1);
    session.dispose();
});

test('successful lock preparation freezes the session', async () => {
    const { saves, session } = setup();
    await session.load('e1');
    session.setField('title', 'Edited');
    const ok = await session.prepareLock();
    assert.equal(ok, true);
    assert.equal(session.frozen, true);
    assert.equal(saves.length, 1);
    session.dispose();
});

test('locking during the last upload chunk aborts the upload before it can commit', async () => {
    const { api, session } = setup();
    await session.load('e1');
    const view = fakeViewApi();
    session.attachEditor(view);
    const chunkStarted = deferred();
    const chunkFinished = deferred();
    const aborted = [];
    let commits = 0;
    api.attachmentChunk = async () => {
        chunkStarted.resolve();
        await chunkFinished.promise;
    };
    api.attachmentAbort = async (id) => { aborted.push(id); };
    api.attachmentFinish = async () => {
        commits++;
        return { id: 'att1', mime: 'image/png', rev: 2 };
    };

    session.attachFiles([fakeFile()]);
    await chunkStarted.promise;
    const locking = session.prepareLock();
    chunkFinished.resolve();

    assert.equal(await locking, true);
    assert.deepEqual(aborted, ['u1']);
    assert.equal(commits, 0);
    assert.deepEqual(view.calls, []);
    session.dispose();
});

for (const transition of ['lock', 'failed lock', 'editor replacement']) {
    test(`a pending link-title refresh is discarded after ${transition}`, async () => {
        const { api, session } = setup();
        await session.load('e1');
        const view = fakeViewApi();
        const detach = session.attachEditor(view);
        const target = deferred();
        const replacement = fakeViewApi();
        api.entryGet = () => target.promise;
        const targetId = '01900000-0000-7000-8000-000000000001';
        session.links.linkAction('refresh', { href: `entry:${targetId}`, from: 0 });

        if (transition === 'editor replacement') {
            detach();
            session.attachEditor(replacement);
        } else {
            if (transition === 'failed lock') {
                session.setField('title', 'Unsaved edit');
                api.entryUpdate = async () => { throw 'revision_conflict'; };
            }
            assert.equal(await session.prepareLock(), transition === 'lock');
        }
        target.resolve({ id: targetId, title: 'New title' });
        await new Promise((resolve) => setImmediate(resolve));

        assert.deepEqual(view.calls, []);
        assert.deepEqual(replacement.calls, []);
        assert.equal(session.links.linkNote, '');
        session.dispose();
    });
}

test('link-title refresh uses the live range in the current editor', async () => {
    const { api, session } = setup();
    await session.load('e1');
    const view = fakeViewApi({ locateLink: () => ({ from: 3, to: 8 }) });
    session.attachEditor(view);
    const targetId = '01900000-0000-7000-8000-000000000001';
    api.entryGet = async () => ({ id: targetId, title: 'New title' });
    session.links.linkAction('refresh', { href: `entry:${targetId}`, from: 0 });
    await new Promise((resolve) => setImmediate(resolve));

    assert.deepEqual(view.calls, [['insertEntryLink', { targetId, label: 'New title', from: 3, to: 8 }]]);
    session.dispose();
});

test('copying the current note uses its live title, stable id and clipboard registration', async () => {
    const id = '01900000-0000-7000-8000-000000000001';
    const copied = [];
    const notifications = [];
    let registered = 0;
    const session = createEntrySession({
        api: {
            entryGet: async () => ({ id, rev: 1, title: 'Old title', entry_date: '2026-11-01', tags: [] }),
            clipboardNoteCopy: async () => { registered++; },
        },
        copyText: async (text) => copied.push(text),
        notifyCopied: (message) => notifications.push(message),
        delay: 10000,
    });
    await session.load(id);
    session.setField('title', 'Updated [title]');
    assert.equal(await session.links.copyNoteLink(), true);
    assert.equal(copied[0], `[Updated \\[title\\]](entry:${id})`);
    assert.equal(registered, 1);
    assert.deepEqual(notifications, ['Internal link copied.']);
    session.dispose();
});

test('copying an untitled note uses its date and reports clipboard failures', async () => {
    const id = '01900000-0000-7000-8000-000000000001';
    let copied;
    let fail = false;
    const errors = [];
    const session = createEntrySession({
        api: {
            entryGet: async () => ({ id, rev: 1, title: '', entry_date: '2026-11-01', tags: [] }),
            clipboardNoteCopy: async () => {},
        },
        notifyError: (message) => errors.push(message),
        copyText: async (text) => {
            if (fail) throw new Error('Clipboard unavailable');
            copied = text;
        },
    });
    await session.load(id);
    assert.equal(await session.links.copyNoteLink(), true);
    assert.equal(copied, `[Untitled — 2026-11-01](entry:${id})`);
    fail = true;
    assert.equal(await session.links.copyNoteLink(), false);
    assert.deepEqual(errors, ['Could not copy the link.']);
    session.dispose();
});

test('frozen notes cannot copy and disposed sessions ignore late copy feedback', async () => {
    const id = '01900000-0000-7000-8000-000000000001';
    const gate = deferred();
    let copied = 0;
    const notifications = [];
    const session = createEntrySession({
        api: {
            entryGet: async () => ({ id, rev: 1, title: 'Title', entry_date: '2026-11-01', tags: [] }),
            clipboardNoteCopy: async () => {},
        },
        copyText: async () => { copied++; await gate.promise; },
        notifyCopied: (message) => notifications.push(message),
        notifyError: (message) => notifications.push(message),
    });
    await session.load(id);
    const copying = session.links.copyNoteLink();
    assert.equal(await session.links.copyNoteLink(), false);
    session.dispose();
    gate.resolve();
    assert.equal(await copying, false);
    assert.deepEqual(notifications, []);
    assert.equal(copied, 1);
    const { session: locked } = setup();
    await locked.load('e1');
    await locked.prepareLock();
    assert.equal(await locked.links.copyNoteLink(), false);
    locked.dispose();
});


test('the toolbar Link action wraps the original selection even after dialog focus moves', async () => {
    const { session } = setup();
    await session.load('e1');
    let selection = { from: 7, to: 12, empty: false };
    const view = fakeViewApi({
        query: () => selection,
        readText: (from, to) => from === 7 && to === 12 ? 'words' : '',
    });
    session.attachEditor(view);
    session.links.openLinkPicker();
    assert.equal(session.links.linkPicker.label, 'words');
    selection = { from: 1, to: 1, empty: true };
    session.links.pickLinkTarget({ id: '01900000-0000-7000-8000-000000000001', title: 'Original title' });
    assert.deepEqual(view.calls[0], ['insertEntryLink', {
        targetId: '01900000-0000-7000-8000-000000000001', label: 'words', from: 7, to: 12,
    }]);
    session.dispose();
});

test('the Link dialog can rename a reference without changing its destination', async () => {
    const { session } = setup();
    await session.load('e1');
    const id = '01900000-0000-7000-8000-000000000001';
    const view = fakeViewApi({
        query: () => ({ from: 10, to: 10, empty: true, link: { from: 7, to: 15, href: `entry:${id}` } }),
        readText: () => 'Old name',
    });
    session.attachEditor(view);
    session.links.openLinkPicker();
    session.links.pickLinkTarget({ id, title: 'Target title', linkLabel: 'My label' });
    assert.deepEqual(view.calls[0], ['insertEntryLink', { targetId: id, label: 'My label', from: 7, to: 15 }]);
    session.dispose();
});

test('bracket links keep their title fallback and accept an optional custom label', async () => {
    const { session } = setup();
    await session.load('e1');
    const view = fakeViewApi();
    const target = { id: '01900000-0000-7000-8000-000000000001', title: 'Target title' };
    session.attachEditor(view);
    session.links.requestBracketPicker(5);
    session.links.pickLinkTarget(target);
    assert.equal(view.calls[0][1].label, 'Target title');
    assert.equal(view.calls[0][1].from, 5);
    assert.equal(view.calls[0][1].to, 7);
    session.links.requestBracketPicker(5);
    session.links.pickLinkTarget({ ...target, linkLabel: 'My label' });
    assert.equal(view.calls[1][1].label, 'My label');
    session.dispose();
});


test('the toolbar Link dialog removes a link at the original range while keeping its text', async () => {
    const { session } = setup();
    await session.load('e1');
    const view = fakeViewApi({
        query: () => ({ from: 10, to: 10, empty: true, link: { from: 7, to: 15, href: 'entry:01900000-0000-7000-8000-000000000001' } }),
        readText: () => 'My label',
    });
    session.attachEditor(view);
    session.links.openLinkPicker();
    assert.equal(session.links.linkPicker.canRemove, true);
    session.links.removePickerLink();
    assert.deepEqual(view.calls[0], ['removeLink', { from: 7, to: 15 }]);
    assert.equal(session.links.linkPicker, null);
    session.dispose();
});

test('Remove link is offered for selected links and omitted for ordinary text', async () => {
    const { session } = setup();
    await session.load('e1');
    let hasLink = false;
    const view = fakeViewApi({
        query: () => ({ from: 3, to: 9, empty: false, hasLink }),
        readText: () => 'Words',
    });
    session.attachEditor(view);
    session.links.openLinkPicker();
    assert.equal(session.links.linkPicker.canRemove, false);
    session.links.removePickerLink();
    assert.equal(view.calls.length, 0);
    hasLink = true;
    session.links.openLinkPicker();
    assert.equal(session.links.linkPicker.canRemove, true);
    session.links.removePickerLink();
    assert.deepEqual(view.calls[0], ['removeLink', { from: 3, to: 9 }]);
    session.dispose();
});
