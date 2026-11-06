// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makePasteHandler, parseMarkdown } from '../index.js';
import { EditorState, TextSelection } from '../state.js';
import { entryLinkMarkdown, outgoingEntryRefs, readEntryLink } from '../../lib/links.js';

const ID = '01900000-0000-7000-8000-000000000001';

function paste(markdown, from, to, text) {
    let state = EditorState.create({ doc: parseMarkdown(markdown) });
    state = state.apply(state.tr().setSelection(new TextSelection(from, to)));
    makePasteHandler()(text, state, (transaction) => {
        state = state.apply(transaction);
    });
    return state.doc;
}

test('copied entry titles preserve Markdown punctuation, entities and Unicode', () => {
    for (const title of ['Plan [draft] *done*', 'A `code` &amp; <tag> title', '日記 🐈 \\ path', '![image](https://example.com)']) {
        const markdown = entryLinkMarkdown(ID.toUpperCase(), title);
        const refs = outgoingEntryRefs(markdown);
        assert.equal(refs.length, 1);
        assert.equal(refs[0].id, ID);
        assert.equal(refs[0].label, title);
    }
    assert.throws(() => entryLinkMarkdown('not-a-uuid', 'title'));
});

test('pasting a copied note into an empty paragraph creates a clickable entry mark', () => {
    const title = 'Plan [draft] *done* &amp;';
    const doc = paste('', 1, 1, entryLinkMarkdown(ID, title));
    assert.equal(doc.textContent, title);
    assert.equal(doc.firstChild.firstChild.marks[0].attrs.href, `entry:${ID}`);
});

test('pasting a copied note replaces selected text without losing surrounding text', () => {
    const doc = paste('Before replace after', 8, 15, entryLinkMarkdown(ID, 'Target'));
    assert.equal(doc.textContent, 'Before Target after');
    const linked = [];
    doc.descendants((node) => {
        if (node.isText && node.marks.some((mark) => mark.type.name === 'link')) linked.push(node.text);
    });
    assert.deepEqual(linked, ['Target']);
});

test('copied links pasted into code remain literal Markdown', () => {
    const markdown = entryLinkMarkdown(ID, 'Target');
    const doc = paste('```\ncode\n```', 1, 1, markdown);
    assert.equal(doc.firstChild.type.name, 'code_block');
    assert.equal(doc.firstChild.textContent, `${markdown}code`);
    assert.equal(doc.firstChild.firstChild.marks.length, 0);
});


test('the Link dialog accepts one copied note link or a canonical internal destination', () => {
    const title = 'Custom [label] &amp;';
    assert.deepEqual(readEntryLink(entryLinkMarkdown(ID, title)), { id: ID, label: title });
    assert.deepEqual(readEntryLink(`entry:${ID}`), { id: ID, label: '' });
    for (const input of [
        `[bad](entry:not-a-uuid)`,
        `[web](https://example.com)`,
        `Before [link](entry:${ID}) after`,
        `[first](entry:${ID}) [second](entry:${ID})`,
        '`[code](entry:' + ID + ')`',
    ]) assert.equal(readEntryLink(input), null);
});
