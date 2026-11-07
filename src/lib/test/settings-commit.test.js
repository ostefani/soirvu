// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
import { test } from "node:test";
import assert from "node:assert/strict";

import { createSettingsCommit } from "../settings-commit.js";

/** One deferred promise per test barrier. */
function deferred() {
    let resolve, reject;
    const promise = new Promise((res, rej) => {
        resolve = res;
        reject = rej;
    });
    return { promise, resolve, reject };
}

/**
 * A fake backend with whole-object replacement semantics: each
 * write stores exactly what it was given. Merging a patch over a
 * stale base therefore reverts whatever the other write changed —
 * the race the coordinator exists to prevent.
 */
function fakeBackend(initial) {
    let stored = { ...initial };
    return {
        snapshot: () => ({ ...stored }),
        set(next) {
            stored = { ...next };
            return Promise.resolve();
        },
        get() {
            return Promise.resolve({ ...stored });
        },
    };
}

test("rapid patches merge against the latest acknowledgement", async () => {
    const backend = fakeBackend({ a: 1, b: 2 });
    const commit = createSettingsCommit(backend.snapshot(), {
        set: (s) => backend.set(s),
        get: () => backend.get(),
    });
    // Two toggles from the same snapshot: the second must merge
    // over the first's write, not over the stale base.
    const first = commit.save({ a: 10 });
    const second = commit.save({ b: 20 });
    await Promise.all([first, second]);
    assert.deepEqual(backend.snapshot(), { a: 10, b: 20 });
});

test("a failed write leaves the acknowledged snapshot for retry", async () => {
    const backend = fakeBackend({ a: 1, b: 2 });
    let failNext = true;
    const commit = createSettingsCommit(backend.snapshot(), {
        set: (s) => (failNext ? Promise.reject("storage") : backend.set(s)),
        get: () => backend.get(),
    });
    await assert.rejects(commit.save({ a: 10 }), (e) => e === "storage");
    // The store is untouched and the next patch still merges
    // against the last-good snapshot.
    assert.deepEqual(backend.snapshot(), { a: 1, b: 2 });
    failNext = false;
    assert.deepEqual(await commit.save({ b: 20 }), { a: 1, b: 20 });
});

test("a folder change racing a save persists both", async () => {
    const backend = fakeBackend({ a: 1, folder: null });
    const gate = deferred();
    let calls = 0;
    const commit = createSettingsCommit(backend.snapshot(), {
        set: (s) => {
            calls++;
            if (calls === 1) return gate.promise.then(() => backend.set(s));
            return backend.set(s);
        },
        get: () => backend.get(),
    });
    // A slow preference write is in flight when the native picker
    // resolves. The folder change arrives as a one-field patch
    // through the same sequence — never as an adopted snapshot —
    // so it merges over the write instead of reverting it.
    const first = commit.save({ a: 10 });
    const folder = commit.save({ folder: "/sync" });
    await Promise.resolve();
    gate.resolve();
    assert.deepEqual(await Promise.all([first, folder]), [
        { a: 10, folder: null },
        { a: 10, folder: "/sync" },
    ]);
    assert.deepEqual(backend.snapshot(), { a: 10, folder: "/sync" });
});

test("a failed reread forces reconciliation before the next write", async () => {
    const backend = fakeBackend({ a: 1 });
    let failGet = false;
    const commit = createSettingsCommit(backend.snapshot(), {
        set: (s) => backend.set(s),
        get: () => (failGet ? Promise.reject("storage") : backend.get()),
    });
    // The write lands but the confirming re-read fails: the base
    // is uncertain, so the save rejects without advancing it.
    failGet = true;
    await assert.rejects(commit.save({ a: 10 }), (e) => e === "storage");
    assert.deepEqual(backend.snapshot(), { a: 10 });
    // The next patch reconciles first instead of merging over the
    // old base and reverting the landed write.
    failGet = false;
    assert.deepEqual(await commit.save({ b: 20 }), { a: 10, b: 20 });
    assert.deepEqual(backend.snapshot(), { a: 10, b: 20 });
});

test("reconciliation failure refuses the write without losing place", async () => {
    const backend = fakeBackend({ a: 1 });
    let failGet = true;
    const commit = createSettingsCommit(backend.snapshot(), {
        set: (s) => backend.set(s),
        get: () => (failGet ? Promise.reject("storage") : backend.get()),
    });
    await assert.rejects(commit.save({ a: 10 }), (e) => e === "storage");
    // Still unreconciled: another patch refuses rather than
    // writing blind over unknown state. Recovery reconciles once
    // and then writes.
    await assert.rejects(commit.save({ b: 20 }), (e) => e === "storage");
    assert.deepEqual(backend.snapshot(), { a: 10 });
    failGet = false;
    assert.deepEqual(await commit.save({ b: 20 }), { a: 10, b: 20 });
});

test("slow writes still serialize in submission order", async () => {
    const backend = fakeBackend({ a: 1 });
    const gate = deferred();
    let calls = 0;
    const commit = createSettingsCommit(backend.snapshot(), {
        set: (s) => {
            calls++;
            if (calls === 1) return gate.promise.then(() => backend.set(s));
            return backend.set(s);
        },
        get: () => backend.get(),
    });
    const first = commit.save({ a: 10 });
    const second = commit.save({ b: 20 });
    // The second patch waits: it must not read the base while the
    // first write is still in flight.
    await Promise.resolve();
    gate.resolve();
    await Promise.all([first, second]);
    assert.deepEqual(backend.snapshot(), { a: 10, b: 20 });
});
