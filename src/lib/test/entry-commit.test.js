// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
import { test } from "node:test";
import assert from "node:assert/strict";

import { createEntryCommit } from "../entry-commit.js";

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
 * A fake store with the backend's contract: a mutation commits
 * only against the current server revision and bumps it,
 * otherwise it fails with revision_conflict. Mirrors `check_base`
 * plus the bump the Tauri command tests pin down.
 */
function fakeStore() {
    let rev = 1;
    const applied = [];
    return {
        get rev() {
            return rev;
        },
        applied,
        save(base, label) {
            if (base !== rev) return Promise.reject("revision_conflict");
            rev += 1;
            applied.push(label);
            return Promise.resolve({ rev, label });
        },
        /** A genuinely foreign writer (sync, another device). */
        foreignAdvance() {
            rev += 1;
        },
    };
}

test("tasks run in order against fresh bases", async () => {
    const commit = createEntryCommit(1);
    const gate = deferred();
    const seen = [];
    let bStarted = false;
    const a = commit.commit(async (base) => {
        seen.push(["a", base]);
        await gate.promise;
        return { rev: base + 1 };
    });
    const b = commit.commit((base) => {
        bStarted = true;
        seen.push(["b", base]);
        return Promise.resolve({ rev: base + 1 });
    });
    // B is queued behind A: nothing of B runs synchronously, and
    // only the gate release lets A finish so B can start.
    assert.equal(bStarted, false);
    gate.resolve();
    await Promise.all([a, b]);
    // B saw A's committed revision: adoption ran in task order,
    // before the next task started, so no older response can
    // replace newer state.
    assert.deepEqual(seen, [
        ["a", 1],
        ["b", 2],
    ]);
    assert.equal(commit.rev, 3);
});

test("two completions from one observed rev both commit", async () => {
    const store = fakeStore();
    // Without the sequence both submissions carry the same base
    // and the backend refuses the second — the reported failure
    // this double reproduces before the fix is exercised below.
    await store.save(1, "first");
    await assert.rejects(store.save(1, "second"), (e) => e === "revision_conflict");

    const fresh = fakeStore();
    const commit = createEntryCommit(fresh.rev);
    // Two uploads read the same entry state, flush, then submit:
    // each task learns its base when it starts instead.
    const first = commit.commit((base) => fresh.save(base, "first"));
    const second = commit.commit((base) => fresh.save(base, "second"));
    await Promise.all([first, second]);
    assert.deepEqual(fresh.applied, ["first", "second"]);
    assert.equal(commit.rev, 3);
});

test("save and finish overlap in either order without loss", async () => {
    for (const [first, second] of [
        ["save", "finish"],
        ["finish", "save"],
    ]) {
        const store = fakeStore();
        const commit = createEntryCommit(store.rev);
        const a = commit.commit((base) => store.save(base, first));
        const b = commit.commit((base) => store.save(base, second));
        await Promise.all([a, b]);
        assert.deepEqual(store.applied, [first, second]);
        assert.equal(commit.rev, 3);
    }
});

test("a foreign advance conflicts without retry or overwrite", async () => {
    const store = fakeStore();
    const commit = createEntryCommit(store.rev);
    store.foreignAdvance();
    // Our base is stale: the commit fails and the sequence keeps
    // the old revision instead of adopting unseen content.
    await assert.rejects(
        commit.commit((base) => store.save(base, "ours")),
        (e) => e === "revision_conflict",
    );
    assert.equal(commit.rev, 1);
    // Retrying against the same stale base fails again — no blind
    // overwrite of the foreign write. Recovery is a reopen, which
    // is a fresh sequence on the current revision.
    await assert.rejects(
        commit.commit((base) => store.save(base, "ours-again")),
        (e) => e === "revision_conflict",
    );
    assert.equal(commit.rev, 1);
    const reopened = createEntryCommit(store.rev);
    const done = await reopened.commit((base) => store.save(base, "after-reopen"));
    assert.equal(done.rev, 3);
});

test("a failed task does not block the sequence", async () => {
    const commit = createEntryCommit(1);
    await assert.rejects(
        commit.commit(() => Promise.reject("locked")),
        (e) => e === "locked",
    );
    const done = await commit.commit((base) => Promise.resolve({ rev: base + 1 }));
    assert.equal(done.rev, 2);
    assert.equal(commit.rev, 2);
});
