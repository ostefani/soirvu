// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import assert from 'node:assert/strict';
import test from 'node:test';
import { parseMarkdown } from '../markdown.js';
import { keymapPlugin } from '../rules.js';
import { EditorState, NodeSelection, TextSelection } from '../state.js';
import { EditorView } from '../view.js';

function atText(markdown, text, offset = 0) {
    const doc = parseMarkdown(markdown);
    let selection;
    doc.descendants((node, pos) => {
        if (node.isTextblock && node.textContent === text) selection = new TextSelection(pos + 1 + offset);
    });
    assert.ok(selection);
    return new EditorState({ doc, selection });
}

function press(state, key, modifiers = {}) {
    const keymap = keymapPlugin(state.schema);
    let next;
    assert.equal(keymap.props.handleKeyDown(state, { key, ...modifiers }, (tr) => {
        next = state.apply(tr);
    }), true);
    return next;
}

test('Backspace lifts the first list item after a preceding paragraph', () => {
    for (const marker of ['- ', '1. ', '- [x] ']) {
        const state = atText(`Above\n\n${marker}item`, 'item');
        const next = press(state, 'Backspace');
        assert.equal(next.doc.toString(), 'doc(paragraph("Above"), paragraph("item"))');
        assert.equal(next.selection.$from(next.doc).parentOffset, 0);
    }
});

test('Backspace and forward Delete remove the neighboring separator', () => {
    const markdown = 'Above\n\n---\n\nBelow';
    for (const [text, offset, key] of [['Above', 5, 'Delete'], ['Below', 0, 'Backspace']]) {
        const next = press(atText(markdown, text, offset), key);
        assert.equal(next.doc.toString(), 'doc(paragraph("Above"), paragraph("Below"))');
        assert.equal(next.selection.$from(next.doc).parent.textContent, text);
        assert.equal(next.selection.$from(next.doc).parentOffset, offset);
    }
});

test('clicking the only separator focuses the editor so either key can delete it', () => {
    for (const key of ['Backspace', 'Delete']) {
        const doc = parseMarkdown('---');
        const state = EditorState.create({ doc });
        const atom = { _pos: 0, _node: doc.firstChild };
        const view = {
            state,
            dom: { contains: (node) => node === atom },
            dispatch(tr) { this.state = this.state.apply(tr); },
            focus() { this.focused = true; },
        };
        let prevented = false;
        EditorView.prototype.onMouseDown.call(view, {
            button: 0,
            target: { closest: () => atom },
            preventDefault() { prevented = true; },
        });
        assert.ok(prevented);
        assert.ok(view.focused);
        assert.ok(view.state.selection.eq(new NodeSelection(0)));
        const next = press(view.state, key);
        assert.equal(next.doc.toString(), 'doc(paragraph)');
        assert.equal(next.selection.from, 1);
    }
});

test('Command–Delete removes text back to the rendered line start', (t) => {
    const state = atText('one two three four', 'one two three four', 13);
    const text = {};
    const selection = {
        isCollapsed: true,
        anchorNode: text,
        anchorOffset: 13,
        focusNode: text,
        modify(alter, direction, granularity) {
            assert.deepEqual([alter, direction, granularity], ['extend', 'backward', 'lineboundary']);
            this.isCollapsed = false;
            this.focusOffset = 4;
        },
        collapse(node, offset) {
            assert.equal(node, text);
            assert.equal(offset, 13);
            this.isCollapsed = true;
        },
    };
    globalThis.document = { getSelection: () => selection };
    t.after(() => { delete globalThis.document; });
    const view = Object.assign(Object.create(EditorView.prototype), {
        state,
        dom: { contains: (node) => node === text },
        editable: () => true,
        posFromDOM: (_, offset) => offset + 1,
        dispatch(tr) { this.state = this.state.apply(tr); },
    });
    let prevented = false;
    view.onKeyDown({
        key: 'Backspace',
        metaKey: true,
        preventDefault() { prevented = true; },
    });
    assert.ok(prevented);
    assert.equal(view.state.doc.textContent, 'one  four');
    assert.equal(view.state.selection.from, 5);

    assert.equal(press(state, 'Backspace', { ctrlKey: true }).doc.textContent, 'one two four');
    selection.isCollapsed = false;
    view.state = atText('above  \nsecond line', 'abovesecond line', 17);
    view.onBeforeInput({ inputType: 'deleteSoftLineBackward', preventDefault() {} });
    assert.equal(view.state.doc.toString(), 'doc(paragraph("above", hard_break))');
});
