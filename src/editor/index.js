// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// The editor component (§6.5): createEditor mounts our own view
// over our own state, commands, rules and history. The only
// npm dependencies are prosemirror-model and markdown-it.
// Svelte talks to the editor through
// the returned object; the editor owns the document in between.
//
// The baseline rule lives in the wiring: onChange fires only for
// transactions that change the document, so opening an entry and
// closing it untouched saves nothing and rewrites nothing.

import { Fragment, Slice } from "prosemirror-model";
import { EditorState, TextSelection, cursorMarks, selectionNear } from "./state.js";
import { EditorView } from "./view.js";
import { historyPlugin } from "./history.js";
import { inputRulesPlugin, keymapPlugin } from "./rules.js";
import { deleteSelectionIn, insertTextIn } from "./commands.js";
import { locateLink, selectionInfo, toolbarCommands, toggleTaskAt } from "./toolbar.js";
import { schema } from "./schema.js";
import { isEntryHref } from "../lib/entry-target.js";
import { findWebLinks, parseMarkdown, serializeMarkdown } from "./markdown.js";

export { schema, parseMarkdown, serializeMarkdown };

function looksLikeMarkdown(text) {
    if (/<https?:\/\/[^<>\s]+>/i.test(text)) return true;
    const formatted = /(^|\n)\s{0,3}(#{1,6}\s|>|```|~~~|- |\d+\.\s)|(\*\*|__)[^*\n]+\1|\[[^\]\n]+\]\(https?:[^)\s]+\)/.test(
        text,
    );
    if (formatted) return true;
    if (!/\]\(entry:/i.test(text)) return false;
    let internalLink = false;
    parseMarkdown(text).descendants((node) => {
        if (node.isText && node.marks.some((mark) => mark.type.name === 'link' && isEntryHref(mark.attrs.href))) internalLink = true;
    });
    return internalLink;
}

function insertClipboardLine(tr, state, line) {
    const links = findWebLinks(line);
    if (!links.length) {
        insertTextIn(tr, state, line);
        return;
    }
    const { schema } = state;
    const marks = cursorMarks(state);
    const nodes = [];
    let offset = 0;
    for (const { index, lastIndex, url } of links) {
        if (index > offset) nodes.push(schema.text(line.slice(offset, index), marks));
        const link = schema.marks.link.create({ href: url });
        nodes.push(schema.text(line.slice(index, lastIndex), link.addToSet(marks)));
        offset = lastIndex;
    }
    if (offset < line.length) nodes.push(schema.text(line.slice(offset), marks));
    const pos = deleteSelectionIn(tr, state);
    tr.replaceRange(pos, pos, new Slice(Fragment.fromArray(nodes), 0, 0));
    tr.setSelection(new TextSelection(pos + line.length));
    if (state.storedMarks) tr.setStoredMarks(state.storedMarks);
}

/** Paste policy (§6.5): plain text goes in as text (newlines as
 *  hard breaks, blank lines as paragraph splits); text that looks
 *  like Markdown is parsed as Markdown. Foreign HTML never enters
 *  the schema. Editor copies explicitly identify their Markdown.
 * @returns {(text: string, state: EditorState, dispatch: (tr: import("./transform.js").Transaction) => void, isMarkdown?: boolean) => void}
 *   A handler that inserts clipboard text at the current selection.
 */
export function makePasteHandler() {
    return (text, state, dispatch, isMarkdown = false) => {
        // Inside a code or raw block the clipboard is literal
        // text: no Markdown parsing, no block surgery — the
        // block holds text and the paste must stay text,
        // newlines included.
        const sel = state.selection;
        if (sel.type === "text") {
            const parent = state.doc.resolve(sel.from).parent;
            if (parent.type === state.schema.nodes.code_block || parent.type === state.schema.nodes.raw_block) {
                const tr = state.tr();
                insertTextIn(tr, state, text);
                dispatch(tr);
                return;
            }
        }
        if (isMarkdown || looksLikeMarkdown(text)) {
            const frag = parseMarkdown(text).content;
            const tr = state.tr();
            const pos = deleteSelectionIn(tr, state);
            const $pos = tr.doc.resolve(pos);
            if ($pos.parent.isTextblock && $pos.parent.content.size === 0 && frag.childCount > 0) {
                // Into an empty block: the pasted blocks take its place.
                const start = $pos.start() - 1;
                tr.replaceRange(start, start + $pos.parent.nodeSize, new Slice(frag, 0, 0));
                tr.setSelection(selectionNear(tr.doc, start + frag.size));
            } else {
                tr.replaceRange(pos, pos, new Slice(frag, 1, 1));
                tr.setSelection(selectionNear(tr.doc, pos + frag.size));
            }
            dispatch(tr);
            return;
        }
        // Plain text. Pieces are computed against intermediate
        // states (positions move as text lands), then their steps
        // are replayed onto a single transaction for dispatch.
        let current = state;
        const steps = [];
        let lastSelection = null;
        const send = (tr) => {
            steps.push(...tr.steps);
            if (tr.selectionSet) lastSelection = tr.selection;
            current = current.apply(tr);
        };
        text.replace(/\r\n?/g, "\n").split("\n\n").forEach((para, i) => {
            if (i > 0) {
                const tr = current.tr();
                const sel = current.selection;
                const $from = current.doc.resolve(sel.from);
                const block = $from.parent;
                const blockStart = $from.start();
                const before = block.type.create(block.attrs, block.content.cut(0, $from.parentOffset));
                const after = current.schema.nodes.paragraph.create(
                    null,
                    block.content.cut($from.parentOffset),
                );
                tr.replaceWith(blockStart - 1, $from.end() + 1, [before, after]);
                tr.setSelection(selectionNear(tr.doc, blockStart + before.nodeSize));
                send(tr);
            }
            para.split("\n").forEach((line, j) => {
                if (j > 0) {
                    const tr = current.tr();
                    const pos = deleteSelectionIn(tr, current);
                    const hardBreak = current.schema.nodes.hard_break;
                    tr.replaceRange(pos, pos, new Slice(Fragment.from(hardBreak.create()), 0, 0));
                    tr.setSelection(selectionNear(tr.doc, pos + 1));
                    send(tr);
                }
                if (line) {
                    const tr = current.tr();
                    insertClipboardLine(tr, current, line);
                    send(tr);
                }
            });
        });
        if (!steps.length) return;
        const tr = state.tr();
        for (const step of steps) tr.addStep(step);
        if (lastSelection) tr.setSelection(lastSelection);
        dispatch(tr);
    };
}

/**
 * @param {HTMLElement} el The element to mount into.
 * @param {object} [options]
 * @param {string} [options.markdown] The entry text, Markdown.
 * @param {(markdown: string) => void} [options.onChange] Called
 *   after every document-changing transaction, with the entry
 *   serialized back to normalized Markdown.
 * @param {(file: File) => void} [options.onAttach] A file was
 *   pasted or dropped (§6.5, Phase 4): the app uploads it and then
 *   calls insertAttachment with the result.
 * @param {(el: HTMLElement, id: string) => void} [options.onAttachmentView]
 *   An attachment block was rendered: the app fills the element
 *   with the right viewer for the attachment's type (§6.4).
 * @param {(href: string, modified: boolean) => void} [options.onOpenLink]
 *   A web link was clicked or a note link was modifier-clicked.
 *   The app routes external links through the native confirmation.
 * @param {(info: { bracketPos: number }) => void} [options.onRequestLinkPicker]
 *   `[[` was typed outside code and raw blocks; the app opens the
 *   link picker, which replaces the bracket pair on pick.
 * @param {() => void} [options.onSelectionChange] Called after
 *   every transaction, doc or selection, so floating UI (the
 *   selection toolbar) can re-render from query().
 */
export function createEditor(el, options = {}) {
    const { markdown = "", onChange = () => {}, onOpenLink = null, onRequestLinkPicker = null, onSelectionChange = null, onAttach = null, onAttachmentView = null } = options;
    let readonly = false;

    const plugins = [inputRulesPlugin(onRequestLinkPicker), keymapPlugin(schema), historyPlugin()];

    const view = new EditorView(el, {
        state: EditorState.create({ doc: parseMarkdown(markdown), plugins }),
        dispatchTransaction(tr) {
            const next = view.state.apply(tr);
            view.updateState(next);
            if (tr.docChanged) view.scrollSelectionIntoView();
            if (tr.docChanged) onChange(serializeMarkdown(next.doc));
            if (onSelectionChange) onSelectionChange();
            hydrateAttachments();
        },
        editable: () => !readonly,
        onTaskToggle: (itemPos) => {
            toggleTaskAt(view.state, (tr) => view.dispatch(tr), itemPos);
        },
        handleClickOn: (href, modified) => {
            if (onOpenLink && href) onOpenLink(href, modified === true);
        },
        onPasteText: makePasteHandler(),
        onAttachFile: (file) => {
            if (onAttach) onAttach(file);
        },
    });

    /** Fills freshly rendered attachment blocks via the app (§6.4). */
    function hydrateAttachments() {
        if (!onAttachmentView) return;
        const blocks = view.dom.querySelectorAll(
            "div.attachment[data-attachment-id]:not([data-hydrated])",
        );
        for (const el of blocks) {
            el.dataset.hydrated = "1";
            onAttachmentView(el, el.getAttribute("data-attachment-id"));
        }
    }
    hydrateAttachments();

    const commands = toolbarCommands(schema);

    return {
        /** Runs a selection-toolbar command by name (toolbar.js).
         *  `arg` reaches commands that take one (setCodeLanguage). */
        exec(name, arg) {
            const command = commands[name];
            if (!command) return false;
            const ok = command(view.state, (tr) => view.dispatch(tr), arg);
            view.focus();
            return ok;
        },
        /** What the selection looks like, for the toolbar. */
        query() {
            return selectionInfo(view.state);
        },
        /** The client rect of a document position (§6.5). */
        coordsAtPos(pos) {
            return view.coordsAtPos(pos);
        },
        /**
         * Inserts an uploaded attachment at the selection (§6.5):
         * images inline, everything else as an attachment block
         * after the current block.
         * @param {{ id: string, mime: string, name: string }} added
         */
        insertAttachment(added) {
            const { state } = view;
            const nodes = state.schema.nodes;
            const tr = state.tr();
            if (added.mime.startsWith("image/")) {
                const img = nodes.image.create({
                    src: `attachment:${added.id}`,
                    alt: added.name || "",
                    title: null,
                });
                tr.replaceWith(state.selection.from, state.selection.to, [img]);
            } else {
                const block = nodes.attachment.create({
                    attachmentId: added.id,
                    name: added.name || "Attachment",
                });
                const $from = state.selection.$from(state.doc);
                const after = $from.depth >= 1 ? $from.after(1) : $from.pos;
                tr.insert(after, [block, nodes.paragraph.create()]);
                tr.setSelection(new TextSelection(after + block.nodeSize + 1));
            }
            view.dispatch(tr);
            view.focus();
        },
        getMarkdown() {
            return serializeMarkdown(view.state.doc);
        },
        /**
         * Reads plain text for link labels and previews.
         * @param {number} from Start position in the current document.
         * @param {number} to Exclusive end position in the current document.
         * @returns {string} The selected text, with block boundaries separated by newlines.
         */
        readText(from, to) {
            return view.state.doc.textBetween(from, to, "\n");
        },
        /**
         * Current range of the link run with this href, re-resolved
         * against the live document (for popover actions after an
         * async gap). Null when the link moved beyond recognition.
         * @param {string} href The expected link destination.
         * @param {number} hintPos A document position expected to touch the link.
         */
        locateLink(href, hintPos) {
            return locateLink(view.state, href, hintPos);
        },
        /**
         * Focuses the editor and selects a range.
         * @param {number} from The anchor position in the current document.
         * @param {number} [to] The head position, defaulting to the anchor.
         */
        setTextSelection(from, to) {
            const tr = view.state.tr();
            tr.setSelection(new TextSelection(from, to ?? from));
            view.dispatch(tr);
            view.focus();
        },
        /** Commits a pending IME composition before navigation. */
        finishComposing() {
            const active = document.activeElement;
            if (active && view.dom.contains(active)) active.blur();
        },
        /**
         * Replaces the document and resets history when switching entries.
         * @param {string} md Markdown for the next entry.
         */
        setMarkdown(md) {
            view.updateState(EditorState.create({ doc: parseMarkdown(md), plugins }));
            if (onSelectionChange) onSelectionChange();
            hydrateAttachments();
        },
        focus() {
            view.focus();
        },
        setReadonly(value) {
            readonly = Boolean(value);
            view.setEditable(!readonly);
        },
        destroy() {
            view.destroy();
        },
    };
}
