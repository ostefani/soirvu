import { test } from "node:test";
import assert from "node:assert/strict";

import { createSettingsCommit } from "../settings-commit.js";

test("preference patches serialize and picker refreshes do not write paths", async () => {
    let release;
    const gate = new Promise((resolve) => { release = resolve; });
    let stored = { a: 1, b: 2, folder: null };
    const writes = [];
    const commit = createSettingsCommit({
        async set(patch) {
            writes.push(patch);
            if (writes.length === 1) await gate;
            stored = { ...stored, ...patch };
        },
        async get() {
            return { ...stored };
        },
    });

    const first = commit.save({ a: 10 });
    const second = commit.save({ b: 20 });
    await Promise.resolve();
    assert.deepEqual(writes, [{ a: 10 }]);
    stored.folder = "/sync";
    const refreshed = commit.refresh();
    release();
    assert.deepEqual(await first, { a: 10, b: 2, folder: "/sync" });
    assert.deepEqual(await second, { a: 10, b: 20, folder: "/sync" });
    assert.deepEqual(await refreshed, { a: 10, b: 20, folder: "/sync" });
    assert.deepEqual(writes, [{ a: 10 }, { b: 20 }]);
});

test("a failed reread cannot revert an already persisted patch", async () => {
    let stored = {};
    let fail = true;
    const commit = createSettingsCommit({
        async set(patch) { stored = { ...stored, ...patch }; },
        async get() {
            if (fail) throw "storage";
            return { ...stored };
        },
    });
    await assert.rejects(commit.save({ a: 10 }), (error) => error === "storage");
    fail = false;
    assert.deepEqual(await commit.save({ b: 20 }), { a: 10, b: 20 });
});
