// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from 'node:test';
import assert from 'node:assert/strict';
import { entryLocationScope, folderPathIds } from '../entry-location.js';

const folders = [
    { id: 'parent', parent_id: null },
    { id: 'child', parent_id: 'parent' },
    { id: 'deep', parent_id: 'child' },
];

test('navigation reveals root or the exact containing folder, without descendants', () => {
    assert.deepEqual(entryLocationScope({ folder_id: null }, folders), { kind: 'root' });
    assert.deepEqual(entryLocationScope({ folder_id: 'deep' }, folders), {
        kind: 'folder', id: 'deep', includeDescendants: false,
    });
    assert.deepEqual(folderPathIds(folders, 'deep'), ['parent', 'child', 'deep']);
});

test('entries with unavailable folder metadata stay reachable through the whole vault', () => {
    assert.deepEqual(entryLocationScope({ folder_id: 'missing' }, folders), { kind: 'all' });
    assert.deepEqual(folderPathIds(folders, 'missing'), []);
});

test('revealing recovered folders terminates at missing ancestors and cycles', () => {
    assert.deepEqual(folderPathIds([{ id: 'child', parent_id: 'missing' }], 'child'), ['child']);
    assert.deepEqual(folderPathIds([{ id: 'a', parent_id: 'b' }, { id: 'b', parent_id: 'a' }], 'a'), ['b', 'a']);
});
