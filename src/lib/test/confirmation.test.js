// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from 'node:test';
import assert from 'node:assert/strict';

globalThis.$state = (value) => value;
const { createConfirmation } = await import('../confirmation.svelte.js');

function deferred() {
    let resolve;
    const promise = new Promise((done) => { resolve = done; });
    return { promise, resolve };
}

test('confirmation submits a pending destructive action only once', async () => {
    const gate = deferred();
    let submitted = 0;
    let completed = 0;
    const confirmation = createConfirmation({
        onConfirm: async () => { submitted++; await gate.promise; },
        onConfirmed: () => { completed++; },
        describeError: () => 'Failed',
    });
    const first = confirmation.confirm();
    assert.equal(confirmation.busy, true);
    assert.equal(await confirmation.confirm(), false);
    assert.equal(submitted, 1);
    gate.resolve();
    assert.equal(await first, true);
    assert.equal(completed, 1);
    assert.equal(confirmation.busy, false);
});

test('confirmation failures keep the dialog retryable with caller-provided text', async () => {
    let fail = true;
    let completed = false;
    const confirmation = createConfirmation({
        onConfirm: async () => { if (fail) throw 'revision_conflict'; },
        onConfirmed: () => { completed = true; },
        describeError: (error) => `Caller message: ${error}`,
    });
    assert.equal(await confirmation.confirm(), false);
    assert.equal(confirmation.error, 'Caller message: revision_conflict');
    assert.equal(completed, false);
    fail = false;
    assert.equal(await confirmation.confirm(), true);
    assert.equal(confirmation.error, '');
    assert.equal(completed, true);
});

test('locking or unmounting retires late confirmation callbacks', async () => {
    const gate = deferred();
    let completed = false;
    const confirmation = createConfirmation({
        onConfirm: () => gate.promise,
        onConfirmed: () => { completed = true; },
        describeError: () => 'Failed',
    });
    const pending = confirmation.confirm();
    confirmation.dispose();
    gate.resolve();
    assert.equal(await pending, false);
    assert.equal(completed, false);
    assert.equal(confirmation.error, '');
});
