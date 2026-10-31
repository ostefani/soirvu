// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Markdown <-> document for the SoirVu editor (DESIGN §6.5).
//
// Parsing: markdown-it tokenizes the whole note. Before
// tokenizing, unsupported TOP-LEVEL blocks (raw HTML blocks,
// footnote definitions — anything with no editor meaning) are cut
// out by their source line ranges and replaced with a fenced block
// whose info string is RAW_INFO; the parser turns those fences into
// raw_block nodes holding the original lines, and the serializer
// writes them back unchanged. Pipe tables, by contrast, are
// first-class nodes (Phase 7): they parse to table rows and cells
// and serialize back normalized. Reference links elsewhere in the
// note still resolve, because the rest of the note is parsed in one pass.
//
// Everything the parser cannot map becomes text or a raw_block.
// Nothing is silently dropped: a construct the spike does not know
// how to keep fails loudly instead (see parseTokensForReal below).

import MarkdownIt from "markdown-it";
import {
    MarkdownParser,
    MarkdownSerializer,
    defaultMarkdownSerializer,
} from "prosemirror-markdown";

import { schema } from "./schema.js";

const RAW_INFO = "soirvu-raw";

// ---------------------------------------------------------------------------
// markdown-it setup

// html: true at the TOKENIZER only, against the letter of §6.5 — the
// spike showed why: with html off, an HTML block tokenizes as a
// paragraph and is reflowed into one line of text on save; with it
// on, HTML blocks are tokens, get captured as raw_blocks below, and
// inline HTML becomes literal text. No HTML ever becomes a node,
// a mark, or DOM — the schema has none. Fidelity and safety both win.
const md = new MarkdownIt({ html: true });

// Task items: a list item whose text starts with "[ ]" or "[x]"
// becomes a task. Runs on the token stream after block parsing.
md.core.ruler.after("block", "soirvu_tasks", (state) => {
    const tokens = state.tokens;
    for (let i = 0; i + 2 < tokens.length; i++) {
        if (tokens[i].type !== "list_item_open") continue;
        const inline = tokens[i + 2];
        if (!inline || inline.type !== "inline") continue;
        const match = /^\[([ xX])\][ ]/.exec(inline.content);
        if (!match) continue;
        tokens[i].attrSet("data-checked", match[1] === " " ? "false" : "true");
        inline.content = inline.content.slice(match[0].length);
        const first = inline.children && inline.children[0];
        if (first && first.type === "text") {
            first.content = first.content.slice(match[0].length);
        }
    }
    return true;
});

// Block attachments: a paragraph holding ONLY [name](attachment:<id>)
// becomes an attachment block. Runs after the inline rule — it needs
// the paragraph's parsed children, which do not exist before it.
md.core.ruler.after("inline", "soirvu_attachments", (state) => {
    const tokens = state.tokens;
    for (let i = 0; i + 2 < tokens.length; i++) {
        if (tokens[i].type !== "paragraph_open") continue;
        const inline = tokens[i + 1];
        const kids = inline && inline.children;
        const only =
            kids &&
            kids.length === 3 &&
            kids[0].type === "link_open" &&
            kids[1].type === "text" &&
            kids[2].type === "link_close";
        if (!only) continue;
        const href = kids[0].attrGet("href") || "";
        if (!href.startsWith("attachment:")) continue;
        const block = new state.Token("attachment_block", "", 0);
        block.attrSet("attachment_id", href.slice("attachment:".length));
        block.attrSet("name", kids[1].content);
        tokens.splice(i, 3, block);
        i -= 1;
    }
    return true;
});

// ---------------------------------------------------------------------------
// Raw capture

// Top-level token types the editor understands. Anything else at the
// top level is kept verbatim as a raw block.
const SUPPORTED_TOP = new Set([
    "paragraph_open",
    "heading_open",
    "blockquote_open",
    "bullet_list_open",
    "ordered_list_open",
    "code_block",
    "fence",
    "hr",
    "table_open",
]);

// Footnote definitions are found by a line scan, not by a plugin:
// markdown-it-footnote deletes a definition whose reference it did
// not recognize (which includes every definition after our own
// serializer has escaped the reference's brackets once) — silent
// loss on save, which a diary cannot accept. A definition here is
// kept whole, whether anything references it or not.
function scanFootnoteDefs(lines, rawAt) {
    let inFence = false;
    for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        if (/^\s{0,3}(```|~~~)/.test(line)) {
            inFence = !inFence;
            continue;
        }
        if (inFence || !/^\[\^[^\]]+\]:/.test(line)) continue;
        let end = i + 1;
        // The definition continues over blank and indented lines.
        while (
            end < lines.length &&
            (lines[end].trim() === "" || /^(\t| {4,})/.test(lines[end]))
        ) {
            end++;
        }
        while (end > i + 1 && lines[end - 1].trim() === "") end--;
        if (!rawAt.has(i)) {
            rawAt.set(i, {
                endLine: end,
                text: lines.slice(i, end).join("\n"),
            });
        }
        i = end - 1;
    }
}

// Cuts raw blocks out of the source and replaces each with a
// soirvu-raw fence holding its exact lines.
function extractRawBlocks(source) {
    const tokens = md.parse(source, {});
    const lines = source.split("\n");
    const rawAt = new Map(); // start line -> {endLine, text}
    scanFootnoteDefs(lines, rawAt);
    let depth = 0;
    for (let i = 0; i < tokens.length; i++) {
        const tok = tokens[i];
        if (depth !== 0) {
            depth += tok.nesting;
            continue;
        }
        // One top-level group: this token and everything until the
        // nesting it opens has closed again.
        let d = 0;
        let j = i;
        let start = Infinity;
        let end = 0;
        do {
            const t = tokens[j];
            d += t.nesting;
            if (t.map) {
                start = Math.min(start, t.map[0]);
                end = Math.max(end, t.map[1]);
            }
            j++;
        } while (d > 0 && j < tokens.length);
        // A group's lines come from the maps inside it — a footnote
        // definition's own wrapper token has no map, its paragraph
        // does; a table's opening token spans the whole table.
        if (!SUPPORTED_TOP.has(tok.type) && start !== Infinity) {
            rawAt.set(start, {
                endLine: end,
                text: lines.slice(start, end).join("\n"),
            });
        }
        i = j - 1;
    }
    if (rawAt.size === 0) return source;
    const out = [];
    for (let line = 0; line < lines.length;) {
        const raw = rawAt.get(line);
        if (raw) {
            // The fence must be longer than any backtick run inside.
            const runs = raw.text.match(/`+/g) || [];
            const longest = runs.reduce((n, r) => Math.max(n, r.length), 0);
            const fence = "`".repeat(Math.max(3, longest + 1));
            out.push(`${fence}${RAW_INFO}`, raw.text, fence, "");
            line = raw.endLine;
        } else {
            out.push(lines[line]);
            line++;
        }
    }
    return out.join("\n");
}

// ---------------------------------------------------------------------------
// Link policy (§6.5): which hrefs may become link marks at all.
// markdown-it already refuses javascript:/vbscript: during tokenizing
// (those links stay literal text); this list also stops data: and
// anything else outside the diary's own world.
function allowedHref(href) {
    return /^(https?:\/\/|mailto:|attachment:)/i.test(href || "");
}

// ---------------------------------------------------------------------------

const parser = new MarkdownParser(schema, md, {
    blockquote: { block: "blockquote" },
    paragraph: { block: "paragraph" },
    list_item: {
        block: "list_item",
        getAttrs: (tok) => ({
            checked:
                tok.attrGet("data-checked") === null
                    ? null
                    : tok.attrGet("data-checked") === "true",
        }),
    },
    bullet_list: {
        block: "bullet_list",
        getAttrs: (_tok, tokens, i) => ({ tight: listIsTight(tokens, i) }),
    },
    ordered_list: {
        block: "ordered_list",
        getAttrs: (tok, tokens, i) => ({
            order: +(tok.attrGet("start") || 1),
            tight: listIsTight(tokens, i),
        }),
    },
    heading: {
        block: "heading",
        getAttrs: (tok) => ({ level: +tok.tag.slice(1) }),
    },
    code_block: { block: "code_block", noCloseToken: true },
    fence: {
        block: "code_block",
        getAttrs: (tok) => ({ params: tok.info || "" }),
        noCloseToken: true,
    },
    hr: { node: "horizontal_rule" },
    table: { block: "table" },
    tr: { block: "table_row" },

    image: {
        node: "image",
        getAttrs: (tok) => ({
            src: tok.attrGet("src"),
            title: tok.attrGet("title") || null,
            alt: tok.content || null,
        }),
    },
    hardbreak: { node: "hard_break" },
    attachment_block: {
        node: "attachment",
        getAttrs: (tok) => ({
            attachmentId: tok.attrGet("attachment_id"),
            name: tok.attrGet("name"),
        }),
    },
    em: { mark: "em" },
    strong: { mark: "strong" },
    s: { mark: "strike" },
    link: {
        mark: "link",
        getAttrs: (tok) => ({
            href: tok.attrGet("href"),
            title: tok.attrGet("title") || null,
        }),
    },
    code_inline: { mark: "code", noCloseToken: true },
});

// The spec system above handles one token at a time; three handlers
// need custom logic, so they are replaced on the built parser.

// A raw fence becomes a raw_block with its original text.
parser.tokenHandlers.fence = (state, tok) => {
    const strip = (s) => (s.endsWith("\n") ? s.slice(0, -1) : s);
    if ((tok.info || "") === RAW_INFO) {
        state.openNode(schema.nodes.raw_block, null);
        state.addText(strip(tok.content));
        state.closeNode();
    } else {
        state.openNode(schema.nodes.code_block, { params: tok.info || "" });
        state.addText(strip(tok.content));
        state.closeNode();
    }
};

// Links outside the policy keep their text but lose the mark. (The
// close token then removes a mark that was never opened — a no-op.)
parser.tokenHandlers.link_open = (state, tok) => {
    const href = tok.attrGet("href");
    if (!allowedHref(href)) return;
    state.openMark(
        schema.marks.link.create({ href, title: tok.attrGet("title") || null }),
    );
};

// Cells: markdown-it puts the inline content directly inside
// th/td, with no paragraph token — but a cell holds exactly
// one paragraph (§6.5), so the handlers open and close both.
function openCell(state, tok, header) {
    state.openNode(schema.nodes.table_cell, { header, align: cellAlign(tok) });
    state.openNode(schema.nodes.paragraph);
}
function closeCell(state) {
    state.closeNode();
    state.closeNode();
}
parser.tokenHandlers.th_open = (state, tok) => openCell(state, tok, true);
parser.tokenHandlers.th_close = (state) => closeCell(state);
parser.tokenHandlers.td_open = (state, tok) => openCell(state, tok, false);
parser.tokenHandlers.td_close = (state) => closeCell(state);

// Wrapper tokens with no node of their own: rows and cells
// open directly under the table.
parser.tokenHandlers.thead_open = () => { };
parser.tokenHandlers.thead_close = () => { };
parser.tokenHandlers.tbody_open = () => { };
parser.tokenHandlers.tbody_close = () => { };

// markdown-it marks a cell's alignment as a style attribute.
function cellAlign(tok) {
    const style = tok.attrGet("style") || "";
    const m = /text-align:\s*(left|center|right)/.exec(style);
    return m ? m[1] : null;
}

// Inline HTML (a stray <b>, an <img onerror>) stays as literal text:
// the schema has no HTML, and the text is what the writer sees.
parser.tokenHandlers.html_inline = (state, tok) => state.addText(tok.content);

// Footnote references have no editor meaning in v1; keep the label
// as literal text. (Definitions are raw blocks, kept whole.)
for (const type of [
    "footnote_ref_open",
    "footnote_ref_close",
    // Definitions are cut out as raw blocks before parsing; these are
    // a safety net only, so an unrecognized footnote shape degrades
    // instead of crashing the editor open.
    "footnote_block_open",
    "footnote_block_close",
    "footnote_open",
    "footnote_close",
    "footnote_anchor",
]) {
    parser.tokenHandlers[type] = () => { };
}
parser.tokenHandlers.footnote_ref = (state, tok) => {
    state.addText(`[^${tok.meta.label}]`);
};

// The upstream prosemirror-markdown rule: markdown-it marks the
// paragraphs of a tight list as hidden.
function listIsTight(tokens, i) {
    while (++i < tokens.length) {
        if (tokens[i].type !== "list_item_open") return tokens[i].hidden;
    }
    return false;
}

// ---------------------------------------------------------------------------

const serializer = new MarkdownSerializer(
    {
        ...defaultMarkdownSerializer.nodes,
        table(state, node) {
            // Flush the previous block's pending close first:
            // the cell rendering below measures state.out, and a
            // flush landing mid-measure would corrupt the row.
            state.write();
            const aligns = { left: ":---", center: ":---:", right: "---:", none: "---" };
            node.forEach((row, _o, rowIndex) => {
                let line = "|";
                row.forEach((cell) => {
                    line += " ";
                    // Measuring means the render must land in out
                    // with nothing else interleaved — but nested in
                    // a quote or a list, the cell's own write would
                    // re-emit the line-prefix delimiter into the
                    // measured segment (a bare newline from the
                    // previous row ends the line, so the delimiter
                    // fires again). Suppress it while measuring and
                    // restore it before the row itself is written.
                    const outer = state.delim;
                    state.delim = "";
                    const start = state.out.length;
                    try {
                        if (cell.firstChild) state.renderInline(cell.firstChild);
                    } finally {
                        state.delim = outer;
                    }
                    const seg = state.out.slice(start);
                    state.out = state.out.slice(0, start);
                    // A literal pipe in cell text would split the
                    // cell on reparse; escape it (markdown-it
                    // reads \| back as |). No lookbehind: the
                    // placeholder dance keeps old Safari happy.
                    line += seg
                        .replace(/\\\|/g, "\u0001")
                        .replace(/\|/g, "\\|")
                        .replace(/\u0001/g, "\\|");
                    line += " |";
                });
                state.write(line);
                state.write("\n");
                if (rowIndex === 0) {
                    let marks = "|";
                    row.forEach((cell) => {
                        marks += " " + aligns[cell.attrs.align || "none"] + " |";
                    });
                    state.write(marks);
                    state.write("\n");
                }
            });
            state.closeBlock(node);
        },
        table_row() { },
        table_cell() { },
        raw_block(state, node) {
            state.write(node.textContent);
            state.closeBlock(node);
        },
        attachment(state, node) {
            state.write(
                `[${node.attrs.name}](attachment:${node.attrs.attachmentId})`,
            );
            state.closeBlock(node);
        },
        list_item(state, node) {
            if (node.attrs.checked == null) {
                state.renderContent(node);
                return;
            }
            const marker = node.attrs.checked ? "[x] " : "[ ] ";
            node.forEach((child, _offset, index) => {
                if (index === 0) {
                    state.write(marker);
                    state.renderInline(child);
                    state.closeBlock(child);
                } else {
                    state.render(child, node, index);
                }
            });
        },
    },
    {
        ...defaultMarkdownSerializer.marks,
        strike: {
            open: "~~",
            close: "~~",
            mixable: true,
            expelEnclosingWhitespace: true,
        },
    },
);

// ---------------------------------------------------------------------------

export function parseMarkdown(source) {
    try {
        return parser.parse(extractRawBlocks(source));
    } catch (err) {
        // A diary must open every note it has ever saved. A construct
        // the renderer cannot map fails the parse with "Token type ...
        // not supported"; rather than refuse to open, keep the whole
        // note as one raw block — every byte preserved, edited as text.
        if (!/^Token type /.test(err.message)) throw err;
        const text = source.replace(/\n+$/, "");
        const raw = schema.node(
            "raw_block",
            null,
            text ? [schema.text(text)] : [],
        );
        return schema.node("doc", null, [raw]);
    }
}

export function serializeMarkdown(doc) {
    return serializer.serialize(doc, { tightLists: true });
}
