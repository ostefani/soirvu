import { test } from 'node:test';
import assert from 'node:assert/strict';

const stateHook = (value) => value;
stateHook.raw = (value) => value;
globalThis.$state = stateHook;

const { createEntrySession, parseTags } = await import('../session.svelte.js');

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
        navCapture() {
            return { from: 0, to: 0 };
        },
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
    assert.equal(session.title, 'Hello');
    assert.equal(session.entryDate, '2026-10-20');
    assert.equal(session.mood, '3');
    assert.deepEqual(session.tags, ['a']);
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

test('failed deletion keeps the draft usable and reports the error', async () => {
    const { api, saves, opened, session } = setup();
    await session.load('e1');
    session.setViewApi(fakeViewApi());
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
    session.setViewApi(fakeViewApi());
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
    session.setViewApi(view);
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
    session.setViewApi(fakeViewApi({ insertAttachment: (info) => inserted.push(info) }));
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
    session.setViewApi(fakeViewApi());
    session.setField('title', 'Edited');
    await session.followEntryLink('e2');
    assert.deepEqual(gate.map((target) => target.id), ['e2']);
    assert.deepEqual(opened, []);
    assert.equal(saves.length, 0);
    assert.equal(session.navError, '');
    assert.equal(session.pendingNavId, null);
    assert.equal(session.navigating, false);
    session.dispose();
});

test('failed workspace navigation preserves the source with a retryable error', async () => {
    let allow = false;
    const { saves, session } = setup({ onOpenEntry: async () => allow });
    await session.load('e1');
    session.setViewApi(fakeViewApi());
    session.setField('title', 'Edited');
    await session.followEntryLink('e2');
    assert.match(session.navError, /Retry to navigate/);
    assert.equal(session.pendingNavId, 'e2');
    assert.equal(session.entry.id, 'e1');
    assert.equal(session.navigating, false);
    assert.equal(saves.length, 0);
    allow = true;
    await session.retryPendingNav();
    assert.equal(session.navError, '');
    assert.equal(session.pendingNavId, null);
    session.dispose();
});

test('link follow stays put while frozen', async () => {
    let calls = 0;
    const { session } = setup({ onOpenEntry: async () => (calls++, true) });
    await session.load('e1');
    session.setViewApi(fakeViewApi());
    assert.equal(await session.prepareLock(), true);
    await session.followEntryLink('e2');
    assert.equal(calls, 0);
    assert.equal(session.navError, '');
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
