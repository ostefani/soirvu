import { test } from "node:test";
import assert from "node:assert/strict";
import { makePasteHandler, parseMarkdown, schema, serializeMarkdown } from "../index.js";
import { newlineInCode, splitBlock } from "../commands.js";
import { EditorState, TextSelection } from "../state.js";

function reopen(doc) {
    return parseMarkdown(serializeMarkdown(doc));
}

test("reopening preserves blank paragraphs, boundary spaces, tabs, and terminal line breaks", () => {
    const { doc, paragraph, hard_break: hardBreak } = schema.nodes;
    const original = doc.create(null, [
        paragraph.create(),
        paragraph.create(null, [
            schema.text(" \tHello  "),
            hardBreak.create(),
            schema.text("  world\t "),
            hardBreak.create(),
            hardBreak.create(),
        ]),
        paragraph.create(null, schema.text(" \t ")),
        paragraph.create(),
        paragraph.create(),
    ]);
    assert.ok(reopen(original).eq(original));
});

test("reopening preserves literal entity text and whitespace around formatted text", () => {
    const { doc, paragraph } = schema.nodes;
    const original = doc.create(null, paragraph.create(null, [
        schema.text("  &#0008203; &#0000010; &#32; &amp; "),
        schema.text(" bold ", [schema.marks.strong.create()]),
        schema.text(" code ", [schema.marks.code.create()]),
        schema.text("  "),
    ]));
    assert.equal(reopen(original).textContent, original.textContent);
});

test("plain-text paste preserves leading and trailing whitespace and runs of blank lines", () => {
    const state = EditorState.create({ doc: parseMarkdown("") });
    const text = "\n\n  first  \n\n\n\n\tlast  \n";
    makePasteHandler()(text, state, (transaction) => {
        const doc = reopen(transaction.doc);
        const paragraphs = [];
        doc.forEach((paragraph) => {
            let line = "";
            paragraph.forEach((child) => {
                line += child.isText ? child.text : "\n";
            });
            paragraphs.push(line);
        });
        assert.equal(paragraphs.join("\n\n"), text);
    });
});

test("blank paragraphs created by Enter survive a save", () => {
    let state = EditorState.create({ doc: parseMarkdown("Hello") });
    state = state.apply(state.tr().setSelection(new TextSelection(6)));
    const dispatch = (transaction) => { state = state.apply(transaction); };
    assert.equal(splitBlock(state, dispatch), true);
    assert.equal(splitBlock(state, dispatch), true);
    assert.equal(reopen(state.doc).childCount, 3);
});

test("leaving a code block keeps the newline already entered", () => {
    let state = EditorState.create({ doc: parseMarkdown("```\nline\n\n```") });
    state = state.apply(state.tr().setSelection(new TextSelection(6)));
    assert.equal(newlineInCode(state, (transaction) => { state = state.apply(transaction); }), true);
    const doc = reopen(state.doc);
    assert.equal(doc.firstChild.textContent, "line\n");
    assert.equal(doc.lastChild.type, schema.nodes.paragraph);
});

test("edited raw blocks retain blank lines at their boundaries", () => {
    const { doc, raw_block: rawBlock } = schema.nodes;
    const original = doc.create(null, rawBlock.create(null, schema.text("\n<div>text</div>\n\n")));
    assert.ok(reopen(original).eq(original));
});
