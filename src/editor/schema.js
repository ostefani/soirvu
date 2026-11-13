// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// The SoirVu editor schema (DESIGN §6.5), including raw blocks, attachments,
// strikethrough, and task items.
//
// The schema is a security boundary (§6.5): there is no HTML node or
// mark, and link/image attributes are validated where they are made
// (see markdown.js for the parser side).

import { Schema } from "prosemirror-model";

// CommonMark specs adapted from prosemirror-markdown 1.13.8.
// See markdown-adapter/LICENSE.
const nodes = {
    doc: {
        content: "block+"
    },
    paragraph: {
        content: "inline*",
        group: "block",
        toDOM() { return ["p", 0]; }
    },
    blockquote: {
        content: "block+",
        group: "block",
        toDOM() { return ["blockquote", 0]; }
    },
    horizontal_rule: {
        group: "block",
        toDOM() { return ["div", ["hr"]]; }
    },
    heading: {
        attrs: { level: { default: 1, validate: "number" } },
        content: "(text | image)*",
        group: "block",
        defining: true,
        toDOM(node) { return ["h" + +node.attrs.level, 0]; }
    },
    code_block: {
        content: "text*",
        group: "block",
        code: true,
        defining: true,
        marks: "",
        attrs: { params: { default: "" } },
        toDOM(node) { return ["pre", node.attrs.params ? { "data-params": node.attrs.params } : {}, ["code", 0]]; }
    },
    ordered_list: {
        content: "list_item+",
        group: "block",
        attrs: { order: { default: 1 }, tight: { default: false } },
        toDOM(node) {
            return ["ol", { start: node.attrs.order == 1 ? null : node.attrs.order,
                    "data-tight": node.attrs.tight ? "true" : null }, 0];
        }
    },
    bullet_list: {
        content: "list_item+",
        group: "block",
        attrs: { tight: { default: false } },
        toDOM(node) { return ["ul", { "data-tight": node.attrs.tight ? "true" : null }, 0]; }
    },
    list_item: {
        content: "paragraph block*",
        defining: true,
        attrs: { checked: { default: null } },
        toDOM: (node) =>
            node.attrs.checked == null
                ? ["li", 0]
                : ["li", { "data-checked": String(node.attrs.checked) }, 0],
    },
    text: {
        group: "inline"
    },
    image: {
        inline: true,
        attrs: {
            src: {},
            alt: { default: null },
            title: { default: null }
        },
        group: "inline",
        draggable: true,
        toDOM(node) { return ["img", node.attrs]; }
    },
    hard_break: {
        inline: true,
        group: "inline",
        selectable: false,
        toDOM() { return ["br"]; }
    },
    // A block the editor does not understand, kept as verbatim text.
    // Content is text so it can be edited as plain text; the node view
    // (Phase 2) shows it as a labelled monospace block.
    raw_block: {
        content: "text*",
        group: "block",
        code: true,
        defining: true,
        toDOM: () => ["pre", { class: "raw-block" }, 0],
    },
    // A block-level attachment (audio, video, PDF): a paragraph that
    // holds only [name](attachment:<uuid>) becomes this node.
    attachment: {
        group: "block",
        atom: true,
        attrs: {
            attachmentId: { default: "" },
            name: { default: "" },
        },
        toDOM: (node) => [
            "div",
            {
                class: "attachment",
                "data-attachment-id": node.attrs.attachmentId,
            },
            node.attrs.name,
        ],
    },
    // Tables (Phase 7, §6.5): GFM pipe tables. A cell holds
    // exactly one paragraph of inline content; the first row's
    // cells are headers. Alignment lives on the cells of the
    // header row and is written back into the delimiter row.
    table: {
        content: "table_row+",
        group: "block",
        toDOM: () => ["table", 0],
    },
    table_row: {
        content: "table_cell+",
        toDOM: () => ["tr", 0],
    },
    table_cell: {
        content: "paragraph",
        attrs: {
            header: { default: false },
            align: { default: null },
        },
        toDOM: (node) => [
            node.attrs.header ? "th" : "td",
            node.attrs.align ? { style: `text-align: ${node.attrs.align}` } : {},
            0,
        ],
    },
};

const marks = {
    em: {
        toDOM() { return ["em"]; }
    },
    strong: {
        toDOM() { return ["strong"]; }
    },
    link: {
        attrs: {
            href: {},
            title: { default: null }
        },
        inclusive: false,
        toDOM(node) { return ["a", node.attrs]; }
    },
    code: {
        code: true,
        toDOM() { return ["code"]; }
    },
    strike: {
        toDOM: () => ["del", 0],
    },
};

export const schema = new Schema({ nodes, marks });
