// SPDX-License-Identifier: MIT
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

test('the macOS policy suppresses stock menus and immediate actions on plain text', () => {
    const policy = readFileSync(new URL('../../../src-tauri/src/context_menu/macos_policy.js', import.meta.url), 'utf8');
    const handlers = new Map();
    runInNewContext(policy, {
        window: {
            addEventListener(type, listener, capture) {
                assert.equal(capture, true);
                handlers.set(type, listener);
            },
        },
    });
    for (const type of ['contextmenu', 'webkitmouseforcewillbegin']) {
        let canceled = false;
        handlers.get(type)({
            target: { closest: () => null },
            preventDefault() { canceled = true; },
        });
        assert.equal(canceled, true, type);
    }
});
