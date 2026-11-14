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

test('pasting a standalone HTTPS URL creates a Markdown link outside code', () => {
    const url = 'https://example.com/path';
    for (const [source, expected] of [
        ['', `<${url}>`],
        ['```\n\n```', '```\n' + url + '\n```'],
    ]) {
        let state = EditorState.create({ doc: parseMarkdown(source) });
        makePasteHandler()(url, state, (tr) => { state = state.apply(tr); });
        assert.equal(serializeMarkdown(state.doc), expected);
    }
});

test('a plain click on a web link delegates opening and cancels browser navigation', () => {
    const href = 'https://example.com/path';
    const link = { getAttribute: () => href };
    const calls = [];
    let prevented = false;
    EditorView.prototype.onClick.call({
        dom: { contains: (node) => node === link },
        handleClickOn: (...args) => calls.push(args),
    }, {
        target: { closest: (selector) => selector === 'a[href]' ? link : null },
        metaKey: false,
        ctrlKey: false,
        preventDefault() { prevented = true; },
    });
    assert.equal(prevented, true);
    assert.deepEqual(calls, [[href, false]]);
});

test('HTTPS URLs in pasted prose and saved notes become links', () => {
    const text = 'See https://example.com, then\nhttps://example.org/help.';
    const hrefs = (doc) => {
        const links = [];
        doc.descendants((node) => {
            for (const mark of node.marks) {
                if (mark.type.name === 'link') links.push(mark.attrs.href);
            }
        });
        return links;
    };
    let state = EditorState.create({ doc: parseMarkdown('') });
    makePasteHandler()(text, state, (tr) => { state = state.apply(tr); });
    const expected = ['https://example.com', 'https://example.org/help'];
    assert.deepEqual(hrefs(state.doc), expected);
    assert.deepEqual(hrefs(parseMarkdown(text)), expected);
    assert.deepEqual(hrefs(parseMarkdown(serializeMarkdown(state.doc))), expected);
    assert.deepEqual(hrefs(parseMarkdown('`https://example.com`')), []);
    assert.deepEqual(hrefs(parseMarkdown('![Example](https://example.com)')), []);
});
