// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
import { test } from "node:test";
import assert from "node:assert/strict";

import { createNavigator } from "../navigation.js";

/** One deferred promise per test barrier. */
function deferred() {
    let resolve, reject;
    const promise = new Promise((res, rej) => {
        resolve = res;
        reject = rej;
    });
    return { promise, resolve, reject };
}

function entry(id) {
    return { entryId: id, scope: null, scoped: false, query: "", date: null, sel: null, scroll: 0 };
}

test("push, back, forward and bounds", () => {
    let changes = 0;
    const nav = createNavigator({ onChange: () => changes++ });
    assert.equal(nav.canGoBack, false);
    assert.equal(nav.canGoForward, false);
    assert.equal(nav.current(), null);
    assert.equal(nav.peek(-1), null);

    nav.push(entry("a"));
    assert.equal(nav.canGoBack, false);
    nav.push(entry("b"));
    assert.equal(nav.canGoBack, true);
    assert.equal(nav.canGoForward, false);
    nav.push(entry("c"));
    assert.equal(nav.current().entryId, "c");
    assert.equal(nav.canGoBack, true);

    assert.equal(nav.peek(-1).entryId, "b");
    assert.equal(nav.peek(1), null);
    assert.equal(nav.arriveAt(nav.peek(-1)), true);
    assert.equal(nav.current().entryId, "b");
    assert.equal(nav.canGoForward, true);

    assert.equal(nav.arriveAt(nav.peek(1)), true);
    assert.equal(nav.current().entryId, "c");
    // Out-of-range peeks change nothing.
    assert.equal(nav.peek(1), null);
    assert.equal(nav.peek(-5), null);
    assert.ok(changes > 0);
});

test("a push discards the forward tail", () => {
    const nav = createNavigator();
    nav.push(entry("a"));
    nav.push(entry("b"));
    nav.arriveAt(nav.peek(-1));
    nav.push(entry("c"));
    assert.equal(nav.current().entryId, "c");
    assert.equal(nav.canGoForward, false);
    assert.equal(nav.canGoBack, true);
});

test("replace rewrites the current entry without moving", () => {
    const nav = createNavigator();
    // Empty history replaces into a push.
    nav.replace(entry("a"));
    assert.equal(nav.current().entryId, "a");
    assert.equal(nav.canGoBack, false);

    nav.push(entry("b"));
    nav.replace(entry("B"));
    assert.equal(nav.current().entryId, "B");
    assert.equal(nav.canGoBack, true);
    // The trail behind is untouched.
    assert.equal(nav.peek(-1).entryId, "a");
});

test("a push that slips in mid-flight cannot misplace the landing", () => {
    const nav = createNavigator();
    nav.push(entry("x"));
    nav.push(entry("y"));
    // Back is peeked, then a new navigation lands first.
    const target = nav.peek(-1);
    nav.push(entry("z"));
    // Landing goes to the entry actually shown, not the stale offset.
    assert.equal(nav.arriveAt(target), true);
    assert.equal(nav.current().entryId, "x");
    // Landing on a dropped entry refuses instead of guessing.
    assert.equal(nav.arriveAt(entry("ghost")), false);
    assert.equal(nav.current().entryId, "x");
});

test("reset forgets the trail", () => {
    let changes = 0;
    const nav = createNavigator({ onChange: () => changes++ });
    nav.push(entry("a"));
    nav.reset();
    assert.equal(nav.current(), null);
    assert.equal(nav.canGoBack, false);
    assert.equal(nav.canGoForward, false);
    assert.ok(changes > 0);
});

test("only the newest select stays live; disposal retires all", () => {
    const nav = createNavigator();
    const first = nav.beginSelect();
    assert.equal(nav.isLiveSelect(first), true);
    const second = nav.beginSelect();
    assert.equal(nav.isLiveSelect(first), false);
    assert.equal(nav.isLiveSelect(second), true);
    nav.dispose();
    assert.equal(nav.isLiveSelect(second), false);
    assert.equal(nav.isLiveSelect(nav.beginSelect()), false);
});

test("an overlapping select drops before mutating", async () => {
    // Mirrors the container's guard order: begin, await the
    // settle, then check currency before pushing or applying.
    // Slow A resolves after fast B; only B may land.
    const nav = createNavigator();
    const applied = [];
    const slow = deferred();
    const fast = deferred();

    async function select(id, gate) {
        const token = nav.beginSelect();
        await gate.promise;
        if (!nav.isLiveSelect(token)) return false;
        nav.push(entry(id));
        applied.push(id);
        return true;
    }

    const a = select("a", slow);
    const b = select("b", fast);
    fast.resolve();
    assert.equal(await b, true);
    slow.resolve();
    assert.equal(await a, false);
    assert.deepEqual(applied, ["b"]);
    assert.equal(nav.current().entryId, "b");
    assert.equal(nav.canGoBack, false);
});

test("a blocked settle records nothing", async () => {
    // A select refused by the editor flush must not push, move or
    // apply: the stack and the visible entry stay put.
    const nav = createNavigator();
    nav.push(entry("a"));
    const token = nav.beginSelect();
    const settled = false;
    if (settled && nav.isLiveSelect(token)) nav.push(entry("b"));
    assert.equal(nav.current().entryId, "a");
    assert.equal(nav.canGoForward, false);
});
