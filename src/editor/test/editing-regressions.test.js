// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from "node:test";
import assert from "node:assert/strict";
import { insertHardBreak, insertTextIn } from "../commands.js";
import { makePasteHandler, parseMarkdown, serializeMarkdown } from "../index.js";
import { EditorState, TextSelection } from "../state.js";
import { toolbarCommands } from "../toolbar.js";
import { EditorView } from "../view.js";

test("Heading 2 preserves soft breaks and selection through a Markdown round trip", () => {
    let state = EditorState.create({ doc: parseMarkdown("onetwo") });
    const dispatch = (tr) => { state = state.apply(tr); };
    dispatch(state.tr().setSelection(new TextSelection(4)));
    assert.equal(insertHardBreak(state, dispatch), true);
    const selection = new TextSelection(8, 1);
    dispatch(state.tr().setSelection(selection));
    assert.equal(toolbarCommands(state.schema).heading2(state, dispatch), true);
    assert.equal(state.doc.firstChild.attrs.level, 2);
    assert.equal(state.doc.firstChild.child(1).type.name, "hard_break");
    assert.ok(state.selection.eq(selection));
    assert.ok(parseMarkdown(serializeMarkdown(state.doc)).eq(state.doc));
});

test("Heading 2 applies to eligible blocks in a selection that includes a list paragraph", () => {
    let state = EditorState.create({ doc: parseMarkdown("Title\n\n- item") });
    const dispatch = (tr) => { state = state.apply(tr); };
    const commands = toolbarCommands(state.schema);
    dispatch(state.tr().setSelection(new TextSelection(1, 12)));
    assert.equal(commands.heading2(state, dispatch), true);
    assert.equal(state.doc.firstChild.type.name, "heading");
    assert.equal(state.doc.lastChild.firstChild.firstChild.type.name, "paragraph");
    dispatch(state.tr().setSelection(new TextSelection(11)));
    assert.equal(commands.heading2(state, () => assert.fail("invalid change")), false);
});

test("paste starting with blank lines replaces the selected text once before the next edit", () => {
    let state = EditorState.create({ doc: parseMarkdown("before selected after") });
    const dispatch = (tr) => { state = state.apply(tr); };
    dispatch(state.tr().setSelection(new TextSelection(8, 16)));
    makePasteHandler()("\n\ninserted", state, dispatch);
    const tr = state.tr();
    insertTextIn(tr, state, "!");
    dispatch(tr);
    assert.equal(state.doc.textBetween(0, state.doc.content.size, "\n\n"), "before \n\ninserted! after");
    assert.ok(parseMarkdown(serializeMarkdown(state.doc)).eq(state.doc));
});

test("inline Markdown paste leaves the caret at the inserted range boundary", () => {
    let state = EditorState.create({ doc: parseMarkdown("before after") });
    const dispatch = (tr) => { state = state.apply(tr); };
    dispatch(state.tr().setSelection(new TextSelection(8)));
    makePasteHandler()("**inserted**", state, dispatch);
    assert.equal(state.selection.from, 16);
    const tr = state.tr();
    insertTextIn(tr, state, "!");
    dispatch(tr);
    assert.equal(state.doc.textContent, "before inserted!after");
});

test("multiline plain paste uses breaks inside a table cell and keeps the other cells", () => {
    let state = EditorState.create({ doc: parseMarkdown("| A | B |\n| --- | --- |\n| C | D |") });
    state = state.apply(state.tr().setSelection(new TextSelection(4, 5)));
    makePasteHandler()("one\n\ntwo", state, (tr) => { state = state.apply(tr); });
    const firstRow = state.doc.firstChild.firstChild;
    assert.equal(firstRow.firstChild.firstChild.childCount, 4);
    assert.equal(firstRow.lastChild.textContent, "B");
    assert.ok(parseMarkdown(serializeMarkdown(state.doc)).eq(state.doc));
});

test("clipboard events apply paste once and a native paste mutation only restores the projection", () => {
    let edits = 0;
    let restores = 0;
    let prevented = 0;
    const view = Object.assign(Object.create(EditorView.prototype), {
        state: EditorState.create({ doc: parseMarkdown("") }),
        editable: () => true,
        onSelectionChange() {},
        onPasteText: makePasteHandler(),
        dispatch(tr) { edits++; this.state = this.state.apply(tr); },
        restoreProjection() { restores++; },
        recoverNativeText() { assert.fail("native paste became a text edit"); },
    });
    const getData = (type) => type === "text/plain" ? "pasted" : "";
    const preventDefault = () => { prevented++; };
    view.onPaste({ clipboardData: { getData }, preventDefault });
    view.onBeforeInput({ inputType: "insertFromPaste", dataTransfer: { getData }, preventDefault });
    view.onNativeInput({ inputType: "insertFromPaste" });
    assert.equal(view.state.doc.textContent, "pasted");
    assert.equal(edits, 1);
    assert.equal(restores, 1);
    assert.equal(prevented, 2);
});

test("replacement input changes the browser's target range at a collapsed caret", () => {
    const node = {};
    const view = Object.assign(Object.create(EditorView.prototype), {
        dom: { contains: (candidate) => candidate === node },
        state: EditorState.create({ doc: parseMarkdown("pasted teh") }),
        editable: () => true,
        posFromDOM: (_node, offset) => offset + 1,
        dispatch(tr) { this.state = this.state.apply(tr); },
    });
    view.state = view.state.apply(view.state.tr().setSelection(new TextSelection(11)));
    let canceled = false;
    view.onBeforeInput({
        inputType: "insertReplacementText",
        data: "the",
        getTargetRanges: () => [{ startContainer: node, startOffset: 7, endContainer: node, endOffset: 10 }],
        preventDefault() { canceled = true; },
    });
    assert.equal(canceled, true);
    assert.equal(view.state.doc.textContent, "pasted the");
});
