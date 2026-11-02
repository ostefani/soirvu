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

test("externally obtained snapshots become the next merge base", async () => {
    const backend = fakeBackend({ a: 1, folder: null });
    const commit = createSettingsCommit(backend.snapshot(), {
        set: (s) => backend.set(s),
        get: () => backend.get(),
    });
    // A native folder pick assigns outside the sequence; without
    // adopting it, the next patch would merge over the pre-pick
    // snapshot and drop the picked folder.
    commit.acknowledge({ a: 1, folder: "/sync" });
    assert.deepEqual(await commit.save({ a: 10 }), { a: 10, folder: "/sync" });
    assert.deepEqual(backend.snapshot(), { a: 10, folder: "/sync" });
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
