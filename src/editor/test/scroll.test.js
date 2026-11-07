// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from 'node:test';
import assert from 'node:assert/strict';
import { caretScrollDelta } from '../scroll.js';

const viewport = { top: 100, bottom: 300 };

test('new lines below the writing viewport scroll up with caret breathing room', () => {
    assert.equal(caretScrollDelta({ top: 290, bottom: 316 }, viewport), 28);
});

test('typing in view does not disturb scroll position', () => {
    assert.equal(caretScrollDelta({ top: 160, bottom: 186 }, viewport), 0);
});

test('editing above the viewport scrolls back to the caret', () => {
    assert.equal(caretScrollDelta({ top: 90, bottom: 116 }, viewport), -22);
});

test('short viewports reduce the margin to keep a whole caret line visible', () => {
    assert.equal(caretScrollDelta({ top: 100, bottom: 126 }, { top: 100, bottom: 126 }), 0);
});
