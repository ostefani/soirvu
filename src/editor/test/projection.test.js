// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import assert from "node:assert/strict";
import test from "node:test";
import { Window } from "happy-dom";
import { insertTextIn } from "../commands.js";
import { historyPlugin, undo } from "../history.js";
import { createEditor, makePasteHandler, parseMarkdown } from "../index.js";
import { EditorState, TextSelection } from "../state.js";
import { toolbarCommands } from "../toolbar.js";
import { EditorView } from "../view.js";

function assertProjection(view) {
    const { doc } = view.state;
    assert.equal(view.dom.childNodes.length, doc.childCount);
    doc.forEach((block, pos, index) => {
        const element = view.dom.childNodes[index];
        assert.equal(element._node, block);
        assert.equal(element._pos, pos);
    });
    const expected = [];
    doc.descendants((node, pos) => {
        if (node.isText) expected.push([node.text, pos]);
    });
    const actual = [];
    const walker = document.createTreeWalker(view.dom, NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) actual.push([node.textContent, node._pos]);
    assert.deepEqual(actual, expected);
}

function mountDOM(t, makeEditor) {
    const window = new Window();
    const keys = ["document", "Node", "NodeFilter"];
    const previous = keys.map((key) => Object.getOwnPropertyDescriptor(globalThis, key));
    for (const key of keys) globalThis[key] = window[key];

    const dom = document.createElement("div");
    document.body.append(dom);
    const editor = makeEditor(dom);
    t.after(async () => {
        editor.destroy();
        await window.close();
        keys.forEach((key, index) => {
            if (previous[index]) Object.defineProperty(globalThis, key, previous[index]);
            else delete globalThis[key];
        });
    });
    editor.focus();
    return { editor, dom, window };
}

function mount(t, source, onUpdate = assertProjection) {
    const { editor: view, dom, window } = mountDOM(t, (dom) => new EditorView(dom, {
        state: EditorState.create({ doc: parseMarkdown(source), plugins: [historyPlugin()] }),
        onPasteText: makePasteHandler(),
        dispatchTransaction(tr) {
            view.updateState(view.state.apply(tr));
            onUpdate(view);
        },
    }));
    assertProjection(view);
    return { view, dom, window };
}

test("rendered block identities and text positions agree with the model after mixed edits", (t) => {
    const { view } = mount(t, "first\n\n*retained* **marked**\n\nlast");
    const dispatch = (tr) => view.dispatch(tr);
    const paste = makePasteHandler();
    const commands = toolbarCommands(view.state.schema);
    const samples = ["a\nb", "**bold**", "x\n\ny", "plain"];
    let seed = 5000;
    function next(size) {
        seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
        return Math.floor(seed / 2 ** 32 * size);
    }

    for (let i = 0; i < 200; i++) {
        const positions = [];
        view.state.doc.descendants((node, pos) => {
            if (node.isTextblock) positions.push(pos + 1 + next(node.content.size + 1));
        });
        dispatch(view.state.tr().setSelection(new TextSelection(positions[next(positions.length)])));
        switch (next(4)) {
            case 0:
                paste(samples[next(samples.length)], view.state, dispatch);
                break;
            case 1:
                commands.heading2(view.state, dispatch);
                break;
            case 2: {
                const tr = view.state.tr();
                insertTextIn(tr, view.state, "x");
                dispatch(tr);
                break;
            }
            case 3:
                undo(view.state, dispatch);
                break;
        }
    }
});

test("composition stays attached through selection and model updates on both sides", (t) => {
    const { view, dom, window } = mount(t, "one\n\n*draft*\n\nlast", (view) => {
        if (!view.composing) assertProjection(view);
    });
    const block = dom.childNodes[1];
    const text = block.firstChild.firstChild;
    const selection = document.getSelection();
    selection.collapse(text, 0);
    dom.dispatchEvent(new window.CompositionEvent("compositionstart"));
    text.textContent = "日本draft";

    const { doc, schema } = view.state;
    const lastStart = doc.content.size - doc.lastChild.nodeSize;
    view.dispatch(view.state.tr().setSelection(new TextSelection(lastStart + 1)));
    assert.equal(dom.childNodes[1], block);
    assert.equal(selection.anchorNode, text);
    view.dispatch(view.state.tr()
        .insert(lastStart + 1, [schema.text("new ")])
        .insert(1, [schema.text("prefix ")]));
    assert.equal(dom.childNodes[1], block);
    assert.equal(document.getSelection().anchorNode, text);
    assert.equal(text._pos, view.state.doc.firstChild.nodeSize + 1);

    selection.collapse(dom.lastChild.firstChild, 0);
    dom.dispatchEvent(new window.CompositionEvent("compositionend"));
    assert.deepEqual(
        Array.from(dom.childNodes, (child) => child.textContent),
        ["prefix one", "日本draft", "new last"],
    );
    assertProjection(view);
});

for (const pasted of ["", "pasted"]) {
    test(`composition commits once ${pasted ? "before pasted text" : "in an empty block"}`, (t) => {
        const { view, dom, window } = mount(t, "");
        if (pasted) {
            const paste = new window.Event("paste", { cancelable: true });
            Object.defineProperty(paste, "clipboardData", {
                value: { getData: (type) => type === "text/plain" ? pasted : "" },
            });
            dom.dispatchEvent(paste);
        }
        const block = dom.firstChild;
        document.getSelection().collapse(block, 0);
        dom.dispatchEvent(new window.CompositionEvent("compositionstart"));
        if (pasted) block.firstChild.textContent = "日本pasted";
        else block.prepend(document.createTextNode("日本"));
        dom.dispatchEvent(new window.InputEvent("input", {
            inputType: "insertCompositionText",
            isComposing: true,
        }));
        assert.equal(view.state.doc.textContent, pasted);

        // Blur must also commit on engines that omit compositionend.
        const target = document.createElement("button");
        document.body.append(target);
        target.focus();
        assert.equal(document.activeElement, target);
        assert.equal(view.composing, false);
        assert.equal(view.compositionBlock, null);
        assert.equal(view.state.doc.textContent, "日本" + pasted);
        const projected = dom.firstChild;
        dom.dispatchEvent(new window.CompositionEvent("compositionend"));
        document.getSelection().collapse(projected.firstChild, 0);
        dom.dispatchEvent(new window.InputEvent("input", { inputType: "insertFromComposition" }));
        assert.equal(dom.firstChild, projected);
        assert.equal(view.state.doc.textContent, "日本" + pasted);
    });
}

test("Heading 2 commits pending composition before replacing the paragraph", (t) => {
    const { editor, dom, window } = mountDOM(t, (dom) => createEditor(dom, { markdown: "draft" }));
    const text = dom.firstChild.firstChild;
    document.getSelection().collapse(text, 0);
    dom.dispatchEvent(new window.CompositionEvent("compositionstart"));
    text.textContent = "日本draft";

    assert.equal(editor.exec("heading2"), true);
    assert.equal(editor.getMarkdown(), "## 日本draft");
    assert.equal(dom.firstChild.tagName, "H2");
    assert.equal(document.activeElement, dom);
    dom.dispatchEvent(new window.CompositionEvent("compositionend"));
    assert.equal(editor.getMarkdown(), "## 日本draft");
});
