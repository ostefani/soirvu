// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import assert from 'node:assert/strict';
import test from 'node:test';
import { parseMarkdown } from '../markdown.js';
import { deleteToLineStart } from '../commands.js';
import { historyPlugin, undo } from '../history.js';
import { isMacLineDelete, keymapPlugin } from '../rules.js';
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

test('Backspace lifts the first list item and keeps the rest in the list', () => {
    const next = press(atText('Above\n\n- first\n- second', 'first'), 'Backspace');
    assert.ok(next.doc.eq(parseMarkdown('Above\n\nfirst\n\n- second')));
    assert.equal(next.selection.$from(next.doc).parent.textContent, 'first');
});

test('Backspace lifts the first paragraph of a quote after ordinary text', () => {
    const next = press(atText('Above\n\n> first\n>\n> second', 'first'), 'Backspace');
    assert.ok(next.doc.eq(parseMarkdown('Above\n\nfirst\n\n> second')));
    assert.equal(next.selection.$from(next.doc).parent.textContent, 'first');
});

test('Backspace after a quote moves to the end of its last line', () => {
    const state = atText('> first\n>\n> > last\n\nBelow', 'Below');
    const next = press(state, 'Backspace');
    const cursor = next.selection.$from(next.doc);
    assert.equal(next.doc, state.doc);
    assert.equal(cursor.parent.textContent, 'last');
    assert.equal(cursor.parentOffset, cursor.parent.content.size);
});

test('Backspace after a list moves to the end of its last item', () => {
    for (const list of ['- first\n- last', '1. first\n2. last']) {
        const state = atText(`${list}\n\nBelow`, 'Below');
        const next = press(state, 'Backspace');
        const cursor = next.selection.$from(next.doc);
        assert.equal(next.doc, state.doc);
        assert.equal(cursor.parent.textContent, 'last');
        assert.equal(cursor.parentOffset, 4);
    }
});

test('Backspace or forward Delete removes an empty paragraph after a quote or list', () => {
    for (const markdown of ['> text', '- text', '1. text']) {
        const before = atText(markdown, 'text', 4);
        const doc = before.doc.type.create(null, [before.doc.firstChild, before.schema.nodes.paragraph.create()]);
        for (const [key, selection] of [
            ['Backspace', new TextSelection(before.doc.content.size + 1)],
            ['Delete', before.selection],
        ]) {
            const next = press(new EditorState({ doc, selection }), key);
            assert.ok(next.doc.eq(before.doc));
            assert.ok(next.selection.eq(before.selection));
        }
    }
});

test('Backspace lifts the first quoted paragraph inside a list item', () => {
    const state = atText('- item\n\n  > quote\n\n- next', 'quote');
    const next = press(state, 'Backspace');
    assert.ok(next.doc.eq(parseMarkdown('- item\n\n  quote\n\n- next')));
    assert.equal(next.selection.$from(next.doc).parent.textContent, 'quote');
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

test('Backspace and forward Delete select an adjacent attachment before removing it', () => {
    const markdown = 'Above\n\n[Attachment](attachment:9b2f2072-8b5d-4c1f-a3e0-5d6b7a8c9d0e)\n\nBelow';
    for (const [text, offset, key] of [['Above', 5, 'Delete'], ['Below', 0, 'Backspace']]) {
        const state = atText(markdown, text, offset);
        const selected = press(state, key);
        assert.equal(selected.doc, state.doc);
        assert.ok(selected.selection.eq(new NodeSelection(state.doc.firstChild.nodeSize)));
        assert.ok(press(selected, key).doc.eq(parseMarkdown('Above\n\nBelow')));
    }
});

test('Backspace at the start of a quote selects a preceding attachment without creating an edit', () => {
    const initial = atText('[Attachment](attachment:9b2f2072-8b5d-4c1f-a3e0-5d6b7a8c9d0e)\n\n> quote', 'quote');
    let state = EditorState.create({ doc: initial.doc, plugins: [historyPlugin()] });
    state = state.apply(state.tr().setSelection(initial.selection));
    const keymap = keymapPlugin(state.schema);
    let transaction;
    assert.equal(keymap.props.handleKeyDown(state, { key: 'Backspace' }, (tr) => { transaction = tr; }), true);
    assert.equal(transaction.docChanged, false);
    const selected = state.apply(transaction);
    assert.ok(selected.selection.eq(new NodeSelection(0)));
    assert.equal(undo(selected), false);
});

test('the view marks a selected attachment and clears the mark when the caret moves', (t) => {
    const state = atText('[Attachment](attachment:9b2f2072-8b5d-4c1f-a3e0-5d6b7a8c9d0e)\n\nBelow', 'Below');
    const attributes = new Map();
    const atom = {
        _node: state.doc.firstChild,
        setAttribute(name, value) { attributes.set(name, value); },
        removeAttribute(name) { attributes.delete(name); },
    };
    const range = { selectNode(node) { assert.equal(node, atom); } };
    const domSelection = {
        rangeCount: 0,
        removeAllRanges() {},
        addRange(value) { assert.equal(value, range); },
        setBaseAndExtent() {},
    };
    globalThis.document = { getSelection: () => domSelection, createRange: () => range };
    t.after(() => { delete globalThis.document; });
    const view = Object.assign(Object.create(EditorView.prototype), {
        state: press(state, 'Backspace'),
        candidates: () => [{ leaf: true, from: 0, node: atom }],
        domPointForPos: () => ({ node: {}, offset: 0 }),
    });
    view.syncSelection();
    assert.ok(attributes.has('data-selected'));
    view.state = state;
    view.syncSelection();
    assert.equal(attributes.has('data-selected'), false);
});

test('character deletion preserves emoji boundaries after an inline atom', () => {
    const markdown = 'a![image](https://example.com/image.png)😀b';
    assert.equal(press(atText(markdown, 'a😀b', 2), 'Delete').doc.firstChild.textContent, 'ab');
    assert.equal(press(atText(markdown, 'a😀b', 4), 'Backspace').doc.firstChild.textContent, 'ab');
    const next = press(atText(markdown, 'a😀b', 2), 'Backspace');
    assert.equal(next.doc.toString(), 'doc(paragraph("a😀b"))');
});

test('Command–Delete routes through the keymap and deletes to the rendered line start on macOS', (t) => {
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
    view.onKeyDown(event);
    assert.ok(prevented);
    assert.deepEqual(received, ['Backspace']);
    assert.equal(view.state.doc.textContent, 'one  four');
    assert.equal(view.state.selection.from, 5);
    assert.equal(press(state, 'Backspace', { ctrlKey: true }).doc.textContent, 'one two four');

    platform.platform = 'Win32';
    assert.equal(isMacLineDelete(event), false);
    delete platform.platform;
    assert.equal(isMacLineDelete(event), false);
});

test('line deletion after a hard break removes the break and keeps the surrounding text', () => {
    const state = atText('above  \nbelow', 'abovebelow', 6);
    let next;
    assert.equal(deleteToLineStart(state, (tr) => { next = state.apply(tr); }), true);
    assert.ok(next.doc.eq(parseMarkdown('abovebelow')));
    assert.equal(next.selection.$from(next.doc).parentOffset, 5);
});

test('line deletion respects a hard break when the line contains an inline atom', (t) => {
    globalThis.document = { getSelection: () => null };
    t.after(() => { delete globalThis.document; });
    const view = Object.assign(Object.create(EditorView.prototype), {
        state: atText('above  \n![image](https://example.com/image.png)second line', 'abovesecond line', 18),
        editable: () => true,
        dispatch(tr) { this.state = this.state.apply(tr); },
    });
    view.onBeforeInput({ inputType: 'deleteSoftLineBackward', preventDefault() {} });
    assert.equal(view.state.doc.toString(), 'doc(paragraph("above", hard_break))');
});

test('line deletion at the start of a list item lifts the item', () => {
    const state = atText('Above\n\n- item', 'item');
    let next;
    assert.equal(deleteToLineStart(state, (tr) => { next = state.apply(tr); }), true);
    assert.ok(next.doc.eq(parseMarkdown('Above\n\nitem')));
});

test('IME keydown bypasses editor shortcuts and Enter link activation', () => {
    const view = {
        activateLink() { assert.fail('IME Enter must not activate a link'); },
        pluginProp() { assert.fail('IME keydown must not dispatch an editor shortcut'); },
    };
    for (const flags of [{ isComposing: true }, { isComposing: false, keyCode: 229 }]) {
        EditorView.prototype.onKeyDown.call(view, { key: 'Enter', ...flags });
    }
});

test('ordinary Backspace reaches the editor keymap', () => {
    const keys = [];
    const view = { pluginProp(_name, event) { keys.push(event.key); } };
    EditorView.prototype.onKeyDown.call(view, { key: 'Backspace', keyCode: 8 });
    assert.deepEqual(keys, ['Backspace']);
});
