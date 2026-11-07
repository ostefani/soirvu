import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createJobQueue } from '../job-queue.svelte.js';

globalThis.$state = Object.assign((value) => value, { raw: (value) => value });

test('the queue limits concurrent work and counts both waiting and active jobs', async () => {
    const gate = Promise.withResolvers();
    const started = [];
    const queue = createJobQueue({
        concurrency: 2,
        async run(item) {
            started.push(item);
            await gate.promise;
        },
    });

    queue.add([1, 2, 3]);
    assert.deepEqual(started, [1, 2]);
    assert.equal(queue.pending, 3);
    gate.resolve();
    await queue.settle();
    assert.deepEqual(started, [1, 2, 3]);
    assert.equal(queue.pending, 0);
});

test('abort drops waiting jobs, rejects new work, and requires a fresh submission after resume', async () => {
    const gate = Promise.withResolvers();
    const started = [];
    const queue = createJobQueue({
        concurrency: 1,
        async run(item) {
            started.push(item);
            await gate.promise;
        },
    });

    queue.add([1, 2, 3]);
    let aborted = false;
    const aborting = queue.abort().then(() => { aborted = true; });
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(aborted, false);
    assert.equal(queue.pending, 1);
    gate.resolve();
    await aborting;
    assert.equal(queue.pending, 0);

    queue.add([4]);
    assert.equal(queue.pending, 0);
    assert.deepEqual(started, [1]);
    queue.resume();
    assert.deepEqual(started, [1]);
    queue.add([5]);
    await queue.settle();
    assert.deepEqual(started, [1, 5]);
    assert.equal(queue.pending, 0);
});
