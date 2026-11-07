// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
import { test } from "node:test";
import assert from "node:assert/strict";

import { createRequestOwner } from "../request-owner.js";

/** One deferred promise per test barrier. */
function deferred() {
    let resolve, reject;
    const promise = new Promise((res, rej) => {
        resolve = res;
        reject = rej;
    });
    return { promise, resolve, reject };
}

test("a token stays current until a newer request begins", () => {
    const owner = createRequestOwner();
    const first = owner.begin();
    assert.equal(owner.current(first), true);
    const second = owner.begin();
    assert.equal(owner.current(first), false);
    assert.equal(owner.current(second), true);
});

test("invalidate retires outstanding tokens without starting a request", () => {
    const owner = createRequestOwner();
    const token = owner.begin();
    owner.invalidate();
    assert.equal(owner.current(token), false);
    // The next request starts fresh rather than inheriting.
    const next = owner.begin();
    assert.equal(owner.current(next), true);
});

test("responses resolving in either order keep only the latest", async () => {
    const owner = createRequestOwner();
    const published = [];
    const slow = deferred();
    const fast = deferred();
    const first = owner.begin();
    const p1 = slow.promise.then((rows) => {
        if (owner.current(first)) published.push(["first", rows]);
    });
    const second = owner.begin();
    const p2 = fast.promise.then((rows) => {
        if (owner.current(second)) published.push(["second", rows]);
    });
    // The newer request resolves first; the older one lands later
    // and must be discarded instead of replacing it.
    fast.resolve(["new"]);
    slow.resolve(["old"]);
    await Promise.all([p1, p2]);
    assert.deepEqual(published, [["second", ["new"]]]);
});

test("a stale failure never clears current data", async () => {
    const owner = createRequestOwner();
    let rows = ["current"];
    let note = "";
    const slow = deferred();
    const first = owner.begin();
    const p1 = slow.promise.catch((e) => {
        if (!owner.current(first)) return;
        rows = [];
        note = String(e);
    });
    const second = owner.begin();
    // The newer request already published; the older failure
    // arrives after and must leave the state alone.
    rows = ["new"];
    slow.reject("storage");
    await p1;
    assert.deepEqual(rows, ["new"]);
    assert.equal(note, "");
    assert.equal(owner.current(second), true);
});

test("disposed owners receive no writes", async () => {
    const owner = createRequestOwner();
    let rows = ["current"];
    const pending = deferred();
    const token = owner.begin();
    const done = pending.promise.then((next) => {
        if (owner.current(token)) rows = next;
    });
    owner.dispose();
    pending.resolve(["late"]);
    await done;
    assert.deepEqual(rows, ["current"]);
    // Requests started after disposal are stillborn too.
    assert.equal(owner.current(owner.begin()), false);
});
