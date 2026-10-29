// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// The SoirVu editor schema (DESIGN §6.5), as a spike: the
// prosemirror-markdown basic schema plus what v1 adds — raw_block,
// attachment, strike, and task items (list_item with a checked attr).
//
// The schema is a security boundary (§6.5): there is no HTML node or
// mark, and link/image attributes are validated where they are made
// (see markdown.js for the parser side).

import { Schema } from "prosemirror-model";
import { schema as base } from "prosemirror-markdown";

const nodes = base.spec.nodes
    // A block the editor does not understand, kept as verbatim text.
    // Content is text so it can be edited as plain text; the node view
    // (Phase 2) shows it as a labelled monospace block.
    .append({
        raw_block: {
            content: "text*",
            group: "block",
            code: true,
            defining: true,
            toDOM: () => ["pre", { class: "raw-block" }, 0],
            parseDOM: [{ tag: "pre.raw-block", preserveWhitespace: "full" }],
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
            parseDOM: [
                {
                    tag: "div.attachment",
                    getAttrs: (dom) => ({
                        attachmentId: dom.getAttribute("data-attachment-id") || "",
                        name: dom.textContent || "",
                    }),
                },
            ],
        },
    })
    // Tables (Phase 7, §6.5): GFM pipe tables. A cell holds
    // exactly one paragraph of inline content; the first row's
    // cells are headers. Alignment lives on the cells of the
    // header row and is written back into the delimiter row.
    .append({
        table: {
            content: "table_row+",
            group: "block",
            toDOM: () => ["table", 0],
            parseDOM: [{ tag: "table" }],
        },
        table_row: {
            content: "table_cell+",
            toDOM: () => ["tr", 0],
            parseDOM: [{ tag: "tr" }],
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
            parseDOM: [
                {
                    tag: "td, th",
                    getAttrs: (dom) => ({
                        header: dom.tagName === "TH",
                        align: dom.style.textAlign || null,
                    }),
                },
            ],
        },
    })
    // Task items: list_item gains an optional checked attribute.
    // null = a plain item, false = [ ], true = [x].
    .update("list_item", {
        content: "paragraph block*",
        defining: true,
        attrs: { checked: { default: null } },
        toDOM: (node) =>
            node.attrs.checked == null
                ? ["li", 0]
                : ["li", { "data-checked": String(node.attrs.checked) }, 0],
        parseDOM: [
            {
                tag: "li",
                getAttrs: (dom) =>
                    dom.hasAttribute("data-checked")
                        ? { checked: dom.getAttribute("data-checked") === "true" }
                        : { checked: null },
            },
        ],
    });

const marks = base.spec.marks.append({
    strike: {
        parseDOM: [
            { tag: "s" },
            { tag: "del" },
            { tag: "strike" },
            {
                style: "text-decoration",
                getAttrs: (value) => (value === "line-through" ? null : false),
            },
        ],
        toDOM: () => ["del", 0],
    },
});

export const schema = new Schema({ nodes, marks });
