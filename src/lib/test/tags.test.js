import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mergeTags, normalizeTags } from '../tags.js';

test('stored comma-separated tags are split and trimmed without dropping duplicates', () => {
    assert.deepEqual(normalizeTags(['a, b', ' c ', '', 'a']), ['a', 'b', 'c', 'a']);
});

test('new tags are deduplicated case-insensitively against existing and incoming tags', () => {
    const current = ['Work', 'home'];
    assert.deepEqual(mergeTags(current, ['work', 'Play', 'play', 'HOME']), ['Work', 'home', 'Play']);
    assert.equal(mergeTags(current, ['WORK', 'HOME']), current);
});
