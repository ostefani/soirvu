// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from 'node:test';
import assert from 'node:assert/strict';
import { makePasteHandler, parseMarkdown, serializeMarkdown } from '../index.js';
import { EditorState, TextSelection } from '../state.js';
import { applyInputRules, inputRulesPlugin, keymapPlugin, undoInputRule } from '../rules.js';
import { insertTextIn } from '../commands.js';
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

    const view = Object.assign(Object.create(EditorView.prototype), {
        state: source,
        readDOMSelection: () => ({ anchor: 14, head: 8 }),
    });
    view.onCopy(event);
    assert.equal(flavors.get('text/plain'), '*italic*');

    const destination = EditorState.create({ doc: parseMarkdown('') });
    let pasted;
    EditorView.prototype.onPaste.call({
        state: destination,
        editable: () => true,
        onPasteText: makePasteHandler(),
        dispatch(transaction) { pasted = destination.apply(transaction); },
    }, event);
    assert.equal(serializeMarkdown(pasted.doc), '*italic*');
    assert.equal(prevented, 2);
});

test('cut writes Markdown and removes the live selection in a transaction', () => {
    const view = Object.assign(Object.create(EditorView.prototype), {
        state: EditorState.create({ doc: parseMarkdown('Before *italic* after') }),
        editable: () => true,
        readDOMSelection: () => ({ anchor: 14, head: 8 }),
        dispatch(tr) { this.state = this.state.apply(tr); },
    });
    const flavors = new Map();
    let prevented = false;
    view.onCut({
        clipboardData: { setData: (type, value) => flavors.set(type, value) },
        preventDefault() { prevented = true; },
    });
    assert.equal(prevented, true);
    assert.equal(flavors.get('text/x-soirvu-markdown'), '*italic*');
    assert.equal(serializeMarkdown(view.state.doc), 'Before  after');
});

test('cut cancels the browser default while composing or read-only', () => {
    for (const [composing, editable] of [[true, true], [false, false]]) {
        let canceled = false;
        EditorView.prototype.onCut.call({ composing, editable: () => editable }, {
            clipboardData: { setData: () => assert.fail('clipboard changed') },
            preventDefault() { canceled = true; },
            stopPropagation() {},
        });
        assert.equal(canceled, true);
    }
});

test('cut cannot fall back to a native DOM mutation when selection mapping fails', () => {
    for (const selection of [null, { anchor: NaN, head: 2 }]) {
        const view = Object.assign(Object.create(EditorView.prototype), {
            editable: () => true,
            readDOMSelection: () => selection,
            dispatch: () => assert.fail('editor mutated'),
        });
        let canceled = false;
        view.onCut({
            clipboardData: { setData: () => assert.fail('clipboard changed') },
            preventDefault() { canceled = true; },
            stopPropagation() {},
        });
        assert.equal(canceled, true);
    }
});

test('pasting a standalone HTTPS URL keeps it plain', () => {
    const url = 'https://example.com/path';
    for (const [source, expected] of [
        ['', url],
        ['```\n\n```', '```\n' + url + '\n```'],
    ]) {
        let state = EditorState.create({ doc: parseMarkdown(source) });
        makePasteHandler()(url, state, (tr) => { state = state.apply(tr); });
        assert.equal(serializeMarkdown(state.doc), expected);
        assert(!state.doc.firstChild.firstChild.marks.some((mark) => mark.type.name === 'link'));
    }
});

test('a plain click on a web link delegates opening and cancels browser navigation', () => {
    const href = 'https://example.com/path';
    const link = { getAttribute: () => href, hasAttribute: () => false };
    const calls = [];
    let prevented = false;
    const view = Object.assign(Object.create(EditorView.prototype), {
        dom: { contains: (node) => node === link },
        handleClickOn: (...args) => calls.push(args),
        readDOMSelection: () => null,
    });
    const event = {
        target: { closest: (selector) => selector === 'a[data-href]' ? link : null },
        metaKey: false,
        ctrlKey: false,
        button: 0,
        detail: 1,
        preventDefault() { prevented = true; },
    };
    view.onClick(event);
    view.onClick({ ...event, detail: 2 });
    view.onClick({ ...event, button: 1 });
    view.readDOMSelection = () => ({ anchor: 1, head: 3 });
    view.onClick(event);
    assert.equal(prevented, true);
    assert.deepEqual(calls, [[href, false]]);
});

test('bare HTTPS URLs stay plain in pasted prose and saved notes', () => {
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
    const expected = [];
    assert.deepEqual(hrefs(state.doc), expected);
    assert.deepEqual(hrefs(parseMarkdown(text)), expected);
    assert.deepEqual(hrefs(parseMarkdown(serializeMarkdown(state.doc))), expected);
    const singleLine = text.replace('\n', ' ');
    assert.equal(serializeMarkdown(parseMarkdown(singleLine)), singleLine);
    assert.deepEqual(hrefs(parseMarkdown('`https://example.com`')), []);
    assert.deepEqual(hrefs(parseMarkdown('![Example](https://example.com)')), []);
});

test('paste keeps inline code literal and links URLs inside headings and table cells', () => {
    const href = 'https://example.com/';
    for (const [markdown, pos, marks] of [
        ['`code`', 3, ['code']],
        ['# Header', 3, ['link']],
        ['| header |\n| --- |\n| cell |', 5, ['link']],
    ]) {
        let state = EditorState.create({ doc: parseMarkdown(markdown) });
        state = state.apply(state.tr().setSelection(new TextSelection(pos)));
        makePasteHandler()(`[label](${href})`, state, (tr) => { state = state.apply(tr); });
        const selected = state.doc.resolve(pos).nodeAfter;
        assert.deepEqual(selected.marks.map((mark) => mark.type.name), marks);
    }
});

test('an angle-bracket URL leaves adjacent HTML as text', () => {
    let state = EditorState.create({ doc: parseMarkdown('') });
    makePasteHandler()('<https://example.com><script>', state, (tr) => { state = state.apply(tr); });
    const paragraph = state.doc.firstChild;
    assert.equal(paragraph.firstChild.marks[0].attrs.href, 'https://example.com');
    assert.equal(paragraph.lastChild.text, '<script>');
    assert.deepEqual(paragraph.lastChild.marks, []);
});

test('typing the closing delimiter renders explicit Markdown links', () => {
    const href = 'https://en.wikipedia.org/wiki/Foo_(bar)';
    for (const source of [`[Article](${href}`, `<${href}`]) {
        const end = source.startsWith('<') ? '>' : ')';
        let state = EditorState.create({ doc: parseMarkdown(source) });
        const pos = source.length + 1;
        state = state.apply(state.tr().setSelection(new TextSelection(pos)));
        const tr = applyInputRules(state, pos, pos, end);
        assert.equal(tr.doc.firstChild.firstChild.marks[0].attrs.href, href);
    }
});

test('typing a link preserves Markdown punctuation inside its URL', () => {
    const href = 'https://example.com/a_b/(part)?q=*_~[]&next=value';
    let state = EditorState.create({ doc: parseMarkdown('') });
    for (const character of `[Article](${href})`) {
        const { from, to } = state.selection;
        let tr = applyInputRules(state, from, to, character);
        if (!tr) {
            tr = state.tr();
            insertTextIn(tr, state, character);
        }
        state = state.apply(tr);
    }
    const expected = parseMarkdown(`[Article](${href})`);
    assert(state.doc.eq(expected));
    assert(state.doc.eq(parseMarkdown(serializeMarkdown(state.doc))));
});

test('URL punctuation survives plain and explicit Markdown round trips', () => {
    const url = 'https://example.com/a_b_c/(part)?q=*_~[]&next=value';
    const plain = parseMarkdown(url + '.');
    assert.equal(parseMarkdown(serializeMarkdown(plain)).textContent, url + '.');
    assert.deepEqual(plain.firstChild.firstChild.marks, []);

    const source = '[Article](https://en.wikipedia.org/wiki/Foo_(bar)).';
    const doc = parseMarkdown(source);
    assert(doc.eq(parseMarkdown(serializeMarkdown(doc))));
    assert.equal(doc.firstChild.lastChild.text, '.');
});

test('mail links activate on click or Enter and note links require a modifier or Enter', () => {
    const note = 'entry:01900000-0000-7000-8000-000000000001';
    let href = 'mailto:a@example.com';
    const link = { getAttribute: () => href };
    const calls = [];
    const view = Object.assign(Object.create(EditorView.prototype), {
        dom: { contains: (node) => node === link },
        readDOMSelection: () => null,
        handleClickOn: (target) => calls.push(target),
    });
    const event = {
        target: { closest: () => link },
        button: 0,
        detail: 1,
        preventDefault() {},
    };
    view.onClick(event);
    view.onKeyDown({ ...event, key: 'Enter' });
    href = note;
    view.onClick(event);
    view.onClick({ ...event, ctrlKey: true });
    view.onKeyDown({ ...event, key: 'Enter' });
    assert.deepEqual(calls, ['mailto:a@example.com', 'mailto:a@example.com', note, note]);
});

test('editor input renders complete link markup delivered in one event', () => {
    const doc = parseMarkdown('');
    const view = Object.assign(Object.create(EditorView.prototype), {
        state: EditorState.create({ doc, plugins: [inputRulesPlugin()] }),
        editable: () => true,
        dispatchTransaction(tr) { this.state = this.state.apply(tr); },
    });
    view.onBeforeInput({
        inputType: 'insertText',
        data: '[test](https://example.com)',
        preventDefault() {},
    });
    assert.equal(view.state.doc.textContent, 'test');
    assert.equal(view.state.doc.firstChild.firstChild.marks[0].attrs.href, 'https://example.com');
});

test('Enter renders a literal link already present at the caret', () => {
    const source = '[test](https://example.com)';
    const doc = parseMarkdown('\\' + source);
    let state = EditorState.create({ doc, plugins: [keymapPlugin(doc.type.schema)] });
    state = state.apply(state.tr().setSelection(new TextSelection(source.length + 1)));
    const view = Object.assign(Object.create(EditorView.prototype), {
        state,
        editable: () => true,
        dispatchTransaction(tr) { this.state = this.state.apply(tr); },
    });
    view.onKeyDown({ key: 'Enter', target: {}, preventDefault() {} });
    assert.equal(view.state.doc.firstChild.textContent, 'test');
    assert.equal(view.state.doc.firstChild.firstChild.marks[0].attrs.href, 'https://example.com');
    assert.equal(view.state.doc.childCount, 2);
});

test('typed links keep outer emphasis and Backspace restores the complete literal', () => {
    const literal = '[x](https://example.com)';
    for (const [delimiter, marks] of [
        ['**', ['strong']],
        ['*', ['em']],
        ['***', ['em', 'strong']],
    ]) {
        const doc = parseMarkdown(delimiter + literal.slice(0, -1) + delimiter);
        let state = EditorState.create({ doc, plugins: [inputRulesPlugin()] });
        state = state.apply(state.tr().setSelection(new TextSelection(literal.length)));
        state = state.apply(applyInputRules(state, literal.length, literal.length, ')'));
        assert.deepEqual(state.doc.firstChild.firstChild.marks.map((m) => m.type.name), [...marks, 'link']);
        assert(undoInputRule(state, (tr) => { state = state.apply(tr); }));
        assert.equal(state.doc.textContent, literal);
        assert.equal(state.selection.from, literal.length + 1);
    }
});

test('legacy HTTP and inline attachment links round trip', () => {
    const source = '[x](http://example.com) and [f](attachment:01900000-0000-7000-8000-000000000001)';
    const doc = parseMarkdown(source);
    const links = doc.firstChild.content.content.flatMap((node) => node.marks.map((mark) => mark.attrs.href));
    assert.deepEqual(links, ['http://example.com', 'attachment:01900000-0000-7000-8000-000000000001']);
    assert(doc.eq(parseMarkdown(serializeMarkdown(doc))));
});

test('link input leaves invalid destinations and literal syntax alone', () => {
    for (const source of [
        '\\[x](https://example.com)',
        '![x](https://example.com)',
        '[x](javascript:alert)',
        '[x](data:text/plain,hello)',
        '[x](https://example.com/' + 'a'.repeat(4096) + ')',
    ]) {
        const state = EditorState.create({ doc: parseMarkdown('') });
        assert.equal(applyInputRules(state, 1, 1, source), null);
    }
    const doc = parseMarkdown('[x](https://example.com "' + 'a'.repeat(4097) + '")');
    assert.equal(doc.firstChild.firstChild.marks[0].attrs.title, null);
});

test('Backspace restores the literal link in paragraphs, headings, and table cells', () => {
    const literal = '[x](https://example.com)';
    const partial = literal.slice(0, -1);
    for (const source of [
        'before ' + partial + ' after',
        '# before ' + partial + ' after',
        '| Header |\n| --- |\n| before ' + partial + ' after |',
        '*before* ' + partial + ' after',
        '`before` ' + partial + ' after',
    ]) {
        const doc = parseMarkdown(source);
        let pos;
        doc.descendants((node, start) => {
            const index = node.isText ? node.text.indexOf(partial) : -1;
            if (index >= 0) pos = start + index + partial.length;
        });
        let state = EditorState.create({ doc, plugins: [inputRulesPlugin()] });
        state = state.apply(state.tr().setSelection(new TextSelection(pos)));
        const literalState = state.apply(state.tr().insert(pos, [state.schema.text(')')]));
        state = state.apply(applyInputRules(state, pos, pos, ')'));
        assert(undoInputRule(state, (tr) => { state = state.apply(tr); }));
        assert(state.doc.eq(literalState.doc), source);
        assert.equal(state.selection.from, pos + 1, source);
    }
});

test('rendering a link exposes only its actual destination in the tooltip', (t) => {
    class TestNode {
        constructor(tag) {
            this.tag = tag;
            this.attributes = new Map();
            this.children = [];
            this.dataset = {};
        }
        setAttribute(name, value) { this.attributes.set(name, String(value)); }
        getAttribute(name) { return this.attributes.get(name) ?? null; }
        appendChild(node) {
            node.parent = this;
            this.children.push(node);
        }
        closest(selector) {
            if (selector === 'a[data-href]' && this.tag === 'a' && this.attributes.has('data-href')) return this;
            return this.parent?.closest(selector) ?? null;
        }
    }
    const globals = { document: globalThis.document, Node: globalThis.Node };
    globalThis.Node = TestNode;
    globalThis.document = {
        createElement: (tag) => new TestNode(tag),
        createTextNode: (value) => new TestNode(value),
    };
    t.after(() => {
        for (const [name, value] of Object.entries(globals)) {
            if (value === undefined) delete globalThis[name];
            else globalThis[name] = value;
        }
    });

    const href = 'https://actual.example/';
    const oversized = 'https://actual.example/' + 'a'.repeat(4096);
    for (const destination of [href, oversized]) {
        const source = `[label](${destination} "https://different.example/")`;
        const doc = parseMarkdown(source);
        const container = new TestNode('p');
        EditorView.prototype.renderContent(doc.firstChild, container, 1);
        const rendered = container.children[0];
        assert.equal(rendered.tag, destination === href ? 'a' : 'span');
        assert.equal(rendered.attributes.get('title'), destination === href ? href : undefined);
        assert.equal(rendered.attributes.get('data-href'), destination === href ? href : undefined);
        assert.equal(rendered.attributes.has('href'), false);
        assert.equal(rendered.children[0].tag, 'label');
        const opened = [];
        const view = Object.assign(Object.create(EditorView.prototype), {
            dom: { contains: (node) => node === rendered },
            readDOMSelection: () => null,
            handleClickOn: (url) => opened.push(url),
        });
        view.onClick({ target: rendered, button: 0, detail: 1, preventDefault() {} });
        assert.deepEqual(opened, destination === href ? [href] : []);
        assert.equal(serializeMarkdown(doc), source);
    }

    const untrusted = `[label](${href}) ![tracking](https://remote.example/pixel.png) <iframe src="https://remote.example/"></iframe>`;
    let pasted = EditorState.create({ doc: parseMarkdown('') });
    makePasteHandler()(untrusted, pasted, (tr) => { pasted = pasted.apply(tr); }, true);
    for (const doc of [parseMarkdown(untrusted), pasted.doc]) {
        const container = new TestNode('p');
        EditorView.prototype.renderContent(doc.firstChild, container, 1);
        function assertInert(node) {
            assert(!['iframe', 'object', 'embed', 'script', 'link'].includes(node.tag));
            for (const name of node.attributes.keys()) {
                assert(['title', 'data-href', 'role', 'tabindex', 'alt', 'data-blocked-image', 'contenteditable'].includes(name), name);
            }
            node.children.forEach(assertInert);
        }
        assertInert(container);
    }
});

test('dragging into or within a link does not open it after the DOM selection collapses', () => {
    const href = 'https://example.com/';
    const link = { getAttribute: () => href };
    const linkTarget = { closest: (selector) => selector === 'a[data-href]' ? link : null };
    const calls = [];
    const view = Object.assign(Object.create(EditorView.prototype), {
        dom: { contains: (node) => node === link },
        readDOMSelection: () => null,
        handleClickOn: (url) => calls.push(url),
    });
    const event = {
        target: linkTarget,
        button: 0,
        detail: 1,
        clientX: 10,
        clientY: 10,
        preventDefault() {},
    };

    view.onMouseDown(event);
    view.trackLinkPress({ ...event, clientX: 30 });
    view.onClick(event);
    view.onMouseDown({ ...event, target: {} });
    view.onClick({ ...event, clientX: 30 });
    assert.deepEqual(calls, []);

    view.onMouseDown(event);
    view.onClick({ ...event, clientX: 11 });
    assert.deepEqual(calls, [href]);
});
