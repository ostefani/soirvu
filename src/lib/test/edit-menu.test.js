// SPDX-License-Identifier: MIT
import assert from 'node:assert/strict';
import test from 'node:test';
import { editMenuRequest } from '../edit-menu.js';

test('the edit menu distinguishes selected text, editable fields and password fields', () => {
    const selection = { isCollapsed: false, containsNode: () => true };
    const text = { closest: () => null };
    const field = {
        tagName: 'INPUT',
        type: 'text',
        selectionStart: 0,
        selectionEnd: 6,
        getBoundingClientRect: () => ({ left: 20, bottom: 40 }),
    };
    field.closest = () => field;
    assert.deepEqual(
        editMenuRequest({ target: text, clientX: 10, clientY: 15 }, null, selection),
        { x: 10, y: 15, copy: true, cut: false, paste: false },
    );
    const event = { target: field, clientX: 0, clientY: 0 };
    assert.deepEqual(editMenuRequest(event, field, selection),
        { x: 20, y: 40, copy: true, cut: true, paste: true });
    field.type = 'password';
    assert.deepEqual(editMenuRequest(event, field, selection),
        { x: 20, y: 40, copy: false, cut: false, paste: true });
    assert.equal(editMenuRequest(event, null, selection), null);
    field.type = 'email';
    field.selectionStart = field.selectionEnd = null;
    const emailSelection = { ...selection, toString: () => 'name@example.test' };
    assert.deepEqual(editMenuRequest(event, field, emailSelection),
        { x: 20, y: 40, copy: true, cut: true, paste: true });
    field.readOnly = true;
    assert.deepEqual(editMenuRequest(event, field, emailSelection),
        { x: 20, y: 40, copy: true, cut: false, paste: false });
    field.disabled = true;
    assert.equal(editMenuRequest(event, field, emailSelection), null);
});
