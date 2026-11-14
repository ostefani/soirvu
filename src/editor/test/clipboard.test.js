// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from 'node:test';
import assert from 'node:assert/strict';
import { makePasteHandler, parseMarkdown, serializeMarkdown } from '../index.js';
import { EditorState } from '../state.js';
import { EditorView } from '../view.js';

test('copy preserves markup in a backward selection and pastes it as Markdown', () => {
    const source = EditorState.create({ doc: parseMarkdown('Before *italic* after') });
    const flavors = new Map();
    let prevented = 0;
    const event = {
        clipboardData: {
            setData: (type, value) => flavors.set(type, value),
            getData: (type) => flavors.get(type) ?? '',
        },
        preventDefault() { prevented++; },
    };

    EditorView.prototype.onCopy.call({
        state: source,
        readDOMSelection: () => ({ anchor: 14, head: 8 }),
    }, event);
    assert.equal(flavors.get('text/plain'), '*italic*');

    const destination = EditorState.create({ doc: parseMarkdown('') });
    let pasted;
    EditorView.prototype.onPaste.call({
        state: destination,
        onPasteText: makePasteHandler(),
        dispatch(transaction) { pasted = destination.apply(transaction); },
    }, event);
    assert.equal(serializeMarkdown(pasted.doc), '*italic*');
    assert.equal(prevented, 2);
});
