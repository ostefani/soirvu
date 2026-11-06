// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Step 8, executable form: the toolbar inserts entry links with
// authoring validation mirroring the parser gate, `linkAt`
// anchors the popover to the exact marked run, removal keeps the
// label, and `[[` opens the picker outside code and raw blocks.

import { test } from "node:test";
import assert from "node:assert/strict";

import { parseMarkdown, serializeMarkdown } from "../index.js";
import { EditorState, TextSelection, cursorMarks } from "../state.js";
import { insertTextIn } from "../commands.js";
import { schema } from "../schema.js";
import { inputRulesPlugin } from "../rules.js";
import { insertEntryLink, linkAt, locateLink, selectionInfo, toolbarCommands } from "../toolbar.js";

const A = "01900000-0000-7000-8000-000000000001";
const B = "01900000-0000-7000-8000-000000000002";

function stateFor(md, selFrom, selTo = selFrom) {
    let state = EditorState.create({ doc: parseMarkdown(md) });
    const tr = state.tr();
    tr.setSelection(new TextSelection(selFrom, selTo));
    return state.apply(tr);
}

function run(cmd, state, arg) {
    let next = state;
    const ok = cmd(state, (tr) => {
        next = state.apply(tr);
    }, arg);
    return { ok, next };
}

test("insertEntryLink wraps a range and rejects non-canonical targets", () => {
    const st = stateFor("hello world", 7, 12);
    const { ok, next } = run(toolbarCommands(schema).insertEntryLink, st, {
        targetId: A,
        label: "world",
    });
    assert.ok(ok);
    assert.equal(serializeMarkdown(next.doc).trim(), `hello [world](entry:${A})`);
    // Canonical form normalizes case, like the parser gate.
    const upper = run(toolbarCommands(schema).insertEntryLink, st, { targetId: A.toUpperCase(), label: "w" });
    assert.ok(upper.ok);
    assert.ok(serializeMarkdown(upper.next.doc).includes(`(entry:${A})`));
    // Suffixes, paths and empty ids refuse without touching the doc.
    for (const bad of [`${A}extra`, "not-a-uuid", "", undefined]) {
        const r = run(toolbarCommands(schema).insertEntryLink, st, { targetId: bad, label: "w" });
        assert.equal(r.ok, false);
        assert.equal(serializeMarkdown(r.next.doc).trim(), "hello world");
    }
    // No label, no link.
    const noLabel = run(toolbarCommands(schema).insertEntryLink, st, { targetId: A, label: "" });
    assert.equal(noLabel.ok, false);
});

test("insertEntryLink refuses code blocks and keeps emphasis", () => {
    const code = stateFor("```\nhello\n```", 1, 6);
    const r = run(toolbarCommands(schema).insertEntryLink, code, { targetId: A, label: "hello" });
    assert.equal(r.ok, false);
    const em = stateFor("hello **brave** new", 7, 16);
    const { ok, next } = run(toolbarCommands(schema).insertEntryLink, em, { targetId: B, label: "brave new" });
    assert.ok(ok);
    const out = serializeMarkdown(next.doc).trim();
    // Emphasis survives (from the range's first node), the label
    // lands under one link mark.
    assert.ok(out.includes(`(entry:${B})`), out);
    assert.ok(out.includes("brave new"), out);
    assert.ok(out.includes("**"), out);
});

test("linkAt anchors the exact marked run under an empty cursor", () => {
    // "one" occupies 1..4 inside its paragraph.
    const st = stateFor(`[one](entry:${A}) and [two](entry:${B})`, 2);
    const at = linkAt(st);
    assert.deepEqual(at, { href: `entry:${A}`, from: 1, to: 4 });
    // A boundary cursor still anchors (arrow-keyed onto the edge);
    // plain text reports nothing.
    assert.deepEqual(linkAt(stateFor(`[one](entry:${A}) and more`, 4)), {
        href: `entry:${A}`,
        from: 1,
        to: 4,
    });
    assert.equal(linkAt(stateFor(`[one](entry:${A}) and more`, 6)), null);
    // A range selection reports nothing: ranges author, cursors inspect.
    assert.equal(linkAt(stateFor(`[one](entry:${A})`, 1, 4)), null);
});

test("locateLink only trusts the run under the hint", () => {
    const href = `entry:${A}`;
    const st = stateFor(`[one](${href}) tail`, 2);
    assert.deepEqual(locateLink(st, href, 1), { from: 1, to: 4 });
    // Typing before the link shifts it: the stale hint no longer
    // touches the run, so the operation cancels instead of adopting
    // whatever happens to share the destination.
    const typeTr = st.tr();
    typeTr.replaceWith(1, 1, [schema.text("hey ")]);
    const moved = st.apply(typeTr);
    assert.ok(serializeMarkdown(moved.doc).includes(`[one](${href})`));
    assert.equal(locateLink(moved, href, 1), null);
    // The fresh position still resolves.
    assert.deepEqual(locateLink(moved, href, 6), { from: 5, to: 8 });
    // Deleting the run, pointing far away, sharing the target
    // across two runs, or an unknown href all resolve nothing.
    const gone = st.apply(st.tr().delete(1, 4));
    assert.equal(locateLink(gone, href, 1), null);
    assert.equal(locateLink(st, href, 50), null);
    const dup = stateFor(`[one](${href}) [two](${href})`, 2);
    assert.equal(locateLink(dup, href, 50), null);
    assert.equal(locateLink(st, `entry:${B}`, 1), null);
});

test("removeLink drops the mark and keeps the label", () => {
    const st = stateFor(`[one](entry:${A}) tail`, 1, 4);
    const { ok, next } = run(toolbarCommands(schema).removeLink, st, {});
    assert.ok(ok);
    assert.equal(serializeMarkdown(next.doc).trim(), "one tail");
});

test("typing [[ outside code opens the picker; code stays literal", () => {
    const calls = [];
    const plugin = inputRulesPlugin((info) => calls.push(info));
    const props = plugin.props;
    const dispatch = () => {};
    // "x [" + "[" -> the pair spans 3..5, bracketPos 3.
    let st = EditorState.create({ doc: parseMarkdown("x [") });
    st = st.apply(((tr) => (tr.setSelection(new TextSelection(4)), tr))(st.tr()));
    props.handleTextInput(st, 4, 4, "[", dispatch);
    assert.deepEqual(calls, [{ bracketPos: 3 }]);
    // Code blocks never trigger.
    calls.length = 0;
    let code = EditorState.create({ doc: parseMarkdown("```\n[\n```") });
    const textStart = 1;
    code = code.apply(((tr) => (tr.setSelection(new TextSelection(textStart + 1)), tr))(code.tr()));
    props.handleTextInput(code, textStart + 1, textStart + 1, "[", dispatch);
    assert.deepEqual(calls, []);
});


test('link menu detects links inside mixed selections and removal preserves other formatting', () => {
    const markdown = `Before **[label](entry:${A})** after`;
    const state = stateFor(markdown, 1, 19);
    assert.equal(selectionInfo(state).hasLink, true);
    const result = run(toolbarCommands(schema).removeLink, state, { from: 1, to: 19 });
    assert.equal(result.ok, true);
    assert.equal(serializeMarkdown(result.next.doc), 'Before **label** after');
    assert.equal(selectionInfo(result.next).hasLink, false);
});


function typeText(state, text) {
    const transaction = state.tr();
    insertTextIn(transaction, state, text);
    return state.apply(transaction);
}

test('typing spaces, symbols and letters after a newly created link stays outside its label', () => {
    const created = run(toolbarCommands(schema).insertEntryLink, stateFor('', 1), { targetId: A, label: 'Target' });
    let next = created.next;
    for (const text of [' ', '!?', ' word', ' 🐈']) next = typeText(next, text);
    const linkText = [];
    next.doc.descendants((node) => {
        if (node.isText && node.marks.some((mark) => mark.type === schema.marks.link)) linkText.push(node.text);
    });
    assert.deepEqual(linkText, ['Target']);
    assert.equal(next.doc.textContent, 'Target !? word 🐈');
    assert.equal(linkAt(next), null);
});

test('existing internal and web links are non-inclusive at both boundaries', () => {
    for (const href of [`entry:${A}`, 'https://example.com']) {
        const markdown = `[Target](${href})`;
        assert.equal(cursorMarks(stateFor(markdown, 1)).some((mark) => mark.type === schema.marks.link), false);
        assert.equal(cursorMarks(stateFor(markdown, 7)).some((mark) => mark.type === schema.marks.link), false);
        assert.equal(cursorMarks(stateFor(markdown, 4)).some((mark) => mark.type === schema.marks.link), true);
        assert.equal(serializeMarkdown(typeText(stateFor(markdown, 7), '!').doc), `[Target](${href})!`);
        const before = typeText(stateFor(markdown, 1), '!');
        assert.equal(before.doc.textContent, '!Target');
        assert.equal(before.doc.firstChild.firstChild.marks.some((mark) => mark.type === schema.marks.link), false);
    }
});

test('link text remains editable inside the label and surrounding formatting still continues', () => {
    const state = stateFor(`**[Target](entry:${A})**`, 4);
    assert.equal(serializeMarkdown(typeText(state, 'X').doc), `**[TarXget](entry:${A})**`);
    const end = stateFor(`**[Target](entry:${A})**`, 7);
    const next = typeText(end, '!');
    assert.equal(next.doc.lastChild.lastChild.text, '!');
    assert.equal(next.doc.lastChild.lastChild.marks.some((mark) => mark.type === schema.marks.strong), true);
    assert.equal(next.doc.lastChild.lastChild.marks.some((mark) => mark.type === schema.marks.link), false);
});


test('the toolbar Link active state follows typing marks instead of popup hit ranges', () => {
    const markdown = `[Target](entry:${A})`;
    const end = stateFor(markdown, 7);
    assert.notEqual(selectionInfo(end).link, null);
    assert.equal(selectionInfo(end).linkActive, false);
    assert.equal(selectionInfo(stateFor(markdown, 4)).linkActive, true);
    const plain = typeText(end, ' plain text');
    assert.equal(selectionInfo(plain).linkActive, false);
    assert.equal(selectionInfo(plain).link, null);
    assert.equal(selectionInfo(plain).hasLink, false);
});
