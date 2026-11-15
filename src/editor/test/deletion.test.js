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

test('Backspace lifts a lone list item with or without a preceding paragraph', () => {
    for (const prefix of ['', 'Above\n\n']) {
        for (const marker of ['- ', '1. ', '- [x] ']) {
            const state = atText(`${prefix}${marker}item`, 'item');
            const next = press(state, 'Backspace');
            assert.ok(next.doc.eq(parseMarkdown(`${prefix}item`)));
            assert.equal(next.selection.$from(next.doc).parentOffset, 0);
        }
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
        const event = {
            button: 0,
            target: { closest: () => atom },
            preventDefault() { prevented = true; },
        };
        EditorView.prototype.onMouseDown.call(view, { ...event, shiftKey: true });
        assert.equal(prevented, false);
        assert.equal(view.state, state);
        EditorView.prototype.onMouseDown.call(view, event);
        assert.ok(prevented);
        assert.ok(view.focused);
        assert.ok(view.state.selection.eq(new NodeSelection(0)));
        const next = press(view.state, key);
        assert.equal(next.doc.toString(), 'doc(paragraph)');
        assert.equal(next.selection.from, 1);
    }
});

test('deleting a selected attachment that is the only block leaves a paragraph', () => {
    const doc = parseMarkdown('[Attachment](attachment:9b2f2072-8b5d-4c1f-a3e0-5d6b7a8c9d0e)');
    assert.equal(doc.firstChild.type.name, 'attachment');
    const state = new EditorState({ doc, selection: new NodeSelection(0) });
    assert.equal(press(state, 'Delete').doc.toString(), 'doc(paragraph)');
});

test('character deletion preserves emoji boundaries after an inline atom', () => {
    const markdown = 'a![image](https://example.com/image.png)😀b';
    assert.equal(press(atText(markdown, 'a😀b', 2), 'Delete').doc.firstChild.textContent, 'ab');
    assert.equal(press(atText(markdown, 'a😀b', 4), 'Backspace').doc.firstChild.textContent, 'ab');
    const next = press(atText(markdown, 'a😀b', 2), 'Backspace');
    assert.equal(next.doc.toString(), 'doc(paragraph("a😀b"))');
});

test('Command–Delete removes text back to the rendered line start', (t) => {
    const platform = { platform: 'MacIntel' };
    const navigatorProperty = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
    Object.defineProperty(globalThis, 'navigator', { configurable: true, value: platform });
    t.after(() => {
        if (navigatorProperty) Object.defineProperty(globalThis, 'navigator', navigatorProperty);
        else delete globalThis.navigator;
    });
    const initial = atText('one two three four', 'one two three four', 13);
    const received = [];
    const observer = {
        props: {
            handleKeyDown(_state, event) { received.push(event.key); },
        },
    };
    const state = new EditorState({
        doc: initial.doc,
        selection: initial.selection,
        plugins: [keymapPlugin(initial.schema), observer],
    });
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
    const event = {
        key: 'Backspace',
        metaKey: true,
        preventDefault() { prevented = true; },
    };
    view.onKeyDown({ ...event, isComposing: true });
    assert.equal(view.state, state);
    assert.deepEqual(received, []);
    view.onKeyDown(event);
    assert.ok(prevented);
    assert.deepEqual(received, ['Backspace']);
    assert.equal(view.state.doc.textContent, 'one  four');
    assert.equal(view.state.selection.from, 5);

    assert.equal(press(state, 'Backspace', { ctrlKey: true }).doc.textContent, 'one two four');
    selection.isCollapsed = false;
    view.state = atText('above  \n![image](https://example.com/image.png)second line', 'abovesecond line', 18);
    view.onBeforeInput({ inputType: 'deleteSoftLineBackward', preventDefault() {} });
    assert.equal(view.state.doc.toString(), 'doc(paragraph("above", hard_break))');

    view.state = atText('Above\n\n- item', 'item');
    view.onKeyDown(event);
    assert.ok(view.state.doc.eq(parseMarkdown('Above\n\nitem')));

    view.state = state;
    platform.platform = 'Win32';
    view.onKeyDown(event);
    assert.equal(view.state.doc.textContent, 'one two four');
});
