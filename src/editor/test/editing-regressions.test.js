// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from "node:test";
import assert from "node:assert/strict";
import { insertTextIn } from "../commands.js";
import { makePasteHandler, parseMarkdown, schema, serializeMarkdown } from "../index.js";
import { EditorState, TextSelection } from "../state.js";
import { toolbarCommands } from "../toolbar.js";
import { EditorView } from "../view.js";

test("Heading 2 preserves soft breaks and selection through a Markdown round trip", () => {
    let state = EditorState.create({ doc: parseMarkdown("") });
    const dispatch = (tr) => { state = state.apply(tr); };
    makePasteHandler()("a\nb", state, dispatch);
    const selection = new TextSelection(4, 1);
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

test("replacement input reads Chromium's plain-text transfer and changes the target range", () => {
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
        data: null,
        dataTransfer: {
            types: ["text/plain"],
            getData: (type) => type === "text/plain" ? "the" : "",
        },
        getTargetRanges: () => [{ startContainer: node, startOffset: 7, endContainer: node, endOffset: 10 }],
        preventDefault() { canceled = true; },
    });
    assert.equal(canceled, true);
    assert.equal(view.state.doc.textContent, "pasted the");
});

test("composition beside a hard break commits once and the final input keeps the projection", (t) => {
    const state = EditorState.create({ doc: parseMarkdown("before\\\nafter") });
    const dom = {};
    const block = { _pos: 0, _node: state.doc.firstChild, parentNode: dom };
    const before = { nodeType: 3, _pos: 1, textContent: "before", parentNode: block };
    const hardBreak = { _pos: 7, _node: block._node.child(1) };
    const after = { nodeType: 3, _pos: 8, textContent: "日本after", parentNode: block };
    block.childNodes = [before, hardBreak, after];
    dom.childNodes = [block];
    dom.contains = (el) => el === block;
    const original = Object.getOwnPropertyDescriptor(globalThis, "document");
    Object.defineProperty(globalThis, "document", {
        configurable: true,
        value: { getSelection: () => ({ anchorNode: after }) },
    });
    t.after(() => {
        if (original) Object.defineProperty(globalThis, "document", original);
        else delete globalThis.document;
    });

    let edits = 0;
    const view = Object.assign(Object.create(EditorView.prototype), {
        dom,
        state,
        composing: true,
        editable: () => true,
        dispatch(tr) {
            edits++;
            this.state = this.state.apply(tr);
            block._node = this.state.doc.firstChild;
        },
        restoreProjection() { assert.fail("restored an aligned text block"); },
    });
    view.onNativeInput({ inputType: "insertCompositionText", isComposing: true });
    assert.equal(edits, 0);
    view.onCompositionEnd();
    view.onNativeInput({ inputType: "insertFromComposition" });
    assert.equal(edits, 1);
    assert.equal(view.state.doc.firstChild.child(1), hardBreak._node);
    assert.equal(view.state.doc.firstChild.lastChild.text, "日本after");
    assert.equal(view.state.selection.from, 10);
});

test("composition commits in its original empty or pasted block after the selection moves", async (t) => {
    for (const pasted of ["", "pasted"]) {
        await t.test(pasted ? "start of pasted text" : "empty block", (t) => {
            const view = Object.assign(Object.create(EditorView.prototype), {
                state: EditorState.create({ doc: schema.nodes.doc.create(null, [
                    schema.nodes.paragraph.create(),
                    schema.nodes.paragraph.create(null, schema.text("next")),
                ]) }),
                editable: () => true,
                onSelectionChange() {},
                onPasteText: makePasteHandler(),
                restoreProjection() { assert.fail("restored DOM during native text recovery"); },
            });
            const dom = { childNodes: [], contains: (el) => dom.childNodes.includes(el) };
            view.dom = dom;
            let anchorNode;
            function project() {
                dom.childNodes = [];
                view.state.doc.forEach((node, pos) => {
                    const block = { _node: node, _pos: pos, parentNode: dom, childNodes: [] };
                    const child = node.content.size
                        ? { nodeType: 3, _pos: pos + 1, textContent: node.textContent, parentNode: block }
                        : { nodeType: 1, _leaf: true, tagName: "BR", parentNode: block };
                    block.childNodes.push(child);
                    dom.childNodes.push(block);
                });
                anchorNode = dom.childNodes[0].childNodes[0];
            }
            view.dispatch = (tr) => {
                view.state = view.state.apply(tr);
                project();
            };
            project();
            const original = Object.getOwnPropertyDescriptor(globalThis, "document");
            Object.defineProperty(globalThis, "document", {
                configurable: true,
                value: { getSelection: () => ({ anchorNode }) },
            });
            t.after(() => {
                if (original) Object.defineProperty(globalThis, "document", original);
                else delete globalThis.document;
            });
            if (pasted) {
                view.onPaste({
                    clipboardData: { getData: (type) => type === "text/plain" ? pasted : "" },
                    preventDefault() {},
                });
            }
            view.onCompositionStart();
            const block = dom.childNodes[0];
            if (pasted) anchorNode.textContent = "日本" + pasted;
            else block.childNodes.unshift({ nodeType: 3, textContent: "日本", parentNode: block });
            view.onNativeInput({ inputType: "insertCompositionText", isComposing: true });
            anchorNode = dom.childNodes[1].childNodes[0];
            view.onCompositionEnd();
            view.onNativeInput({ inputType: "insertFromComposition" });
            assert.equal(view.state.doc.firstChild.textContent, "日本" + pasted);
            assert.equal(view.state.doc.lastChild.textContent, "next");
        });
    }
});
