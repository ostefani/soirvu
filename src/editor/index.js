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
import { deleteSelectionIn, insertTextIn, tryBuild } from "./commands.js";
import { locateLink, selectionInfo, toolbarCommands, toggleTaskAt } from "./toolbar.js";
import { schema } from "./schema.js";
import { isEntryHref } from "../lib/entry-target.js";
import { parseMarkdown, serializeMarkdown } from "./markdown.js";

export { schema, parseMarkdown, serializeMarkdown };

function looksLikeMarkdown(text) {
    if (/<(?:https?:\/\/|mailto:)[^<>\s]+>/i.test(text)) return true;
    if (/\]\(\s*<?(?:https?:\/\/|mailto:)/i.test(text)) return true;
    const formatted = /(^|\n)\s{0,3}(#{1,6}\s|>|```|~~~|- |\d+\.\s)|(\*\*|__)[^*\n]+\1/.test(
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

/** Paste policy (§6.5): plain text goes in as text (newlines as
 *  hard breaks, blank lines as paragraph splits); text that looks
 *  like Markdown is parsed as Markdown. Foreign HTML never enters
 *  the schema. Editor copies explicitly identify their Markdown.
 * @returns {(text: string, state: EditorState, dispatch: (tr: import("./transform.js").Transaction) => void, isMarkdown?: boolean) => void}
 *   A handler that inserts clipboard text at the current selection.
 */
export function makePasteHandler() {
    return (text, state, dispatch, isMarkdown = false) => {
        if (!text) return;
        const { doc, schema, selection } = state;
        const marks = cursorMarks(state);
        const parent = doc.resolve(selection.from).parent;
        const literal = selection.type === "text" && (
            parent.type.spec.code || marks.some((mark) => mark.type === schema.marks.code)
        );
        const replaceSelection = (tr) => {
            if (selection.type === "node" && selection.node(doc).isBlock) {
                const pos = selection.from;
                tr.replaceWith(pos, selection.to(doc), [schema.nodes.paragraph.create()]);
                return pos + 1;
            }
            return deleteSelectionIn(tr, state);
        };
        const plain = () => tryBuild(state, (tr) => {
            const pos = replaceSelection(tr);
            const $pos = tr.doc.resolve(pos);
            const block = $pos.parent;
            const normalized = text.replace(/\r\n?/g, "\n");
            const lines = (value) => {
                const nodes = [];
                value.split("\n").forEach((line, index) => {
                    if (index) nodes.push(schema.nodes.hard_break.create());
                    if (line) nodes.push(schema.text(line, marks));
                });
                return Fragment.fromArray(nodes);
            };
            const parts = normalized.split("\n\n");
            if (parts.length > 1 && block.isTextblock && block.type !== schema.nodes.heading) {
                const before = block.content.cut(0, $pos.parentOffset);
                const after = block.content.cut($pos.parentOffset);
                const blocks = parts.map((part, index) => {
                    const inline = lines(part);
                    if (index === 0) return block.copy(before.append(inline));
                    return schema.nodes.paragraph.create(null,
                        index === parts.length - 1 ? inline.append(after) : inline);
                });
                const container = $pos.node(-1);
                const index = $pos.index(-1);
                const fragment = Fragment.fromArray(blocks);
                if (container.canReplace(index, index + 1, fragment)) {
                    tr.replaceRange($pos.before(), $pos.after(), new Slice(fragment, 0, 0));
                    tr.setSelection(new TextSelection($pos.before() + fragment.size - after.size - 1));
                    return;
                }
            }
            const inline = lines(normalized);
            if (block.isTextblock) {
                tr.replaceRange(pos, pos, new Slice(inline, 0, 0));
                tr.setSelection(new TextSelection(pos + inline.size));
            } else {
                const paragraph = schema.nodes.paragraph.create(null, inline);
                tr.insert(pos, [paragraph]);
                tr.setSelection(new TextSelection(pos + 1 + inline.size));
            }
        });
        const markdown = () => tryBuild(state, (tr) => {
            const frag = parseMarkdown(text).content;
            const pos = replaceSelection(tr);
            const $pos = tr.doc.resolve(pos);
            if (!$pos.parent.isTextblock) {
                tr.replaceRange(pos, pos, new Slice(frag, 0, 0));
                tr.setSelection(selectionNear(tr.doc, pos + frag.size - 1));
                return;
            }
            const block = $pos.parent;
            const container = $pos.node(-1);
            const index = $pos.index(-1);
            if (!block.content.size && container.canReplace(index, index + 1, frag)) {
                const start = $pos.before();
                tr.replaceRange(start, $pos.after(), new Slice(frag, 0, 0));
                tr.setSelection(selectionNear(tr.doc, start + frag.size - 1));
                return;
            }
            const paragraph = schema.nodes.paragraph;
            if (frag.firstChild?.type === paragraph && frag.lastChild?.type === paragraph) {
                const slice = new Slice(frag, 1, 1);
                tr.replaceRange(pos, pos, slice);
                tr.setSelection(new TextSelection(pos + slice.size));
                return;
            }
            const before = block.content.cut(0, $pos.parentOffset);
            const after = block.content.cut($pos.parentOffset);
            const prefix = before.size ? Fragment.from(block.copy(before)) : Fragment.empty;
            const suffix = after.size ? Fragment.from(block.copy(after)) : Fragment.empty;
            const content = prefix.append(frag).append(suffix);
            const start = $pos.before();
            tr.replaceRange(start, $pos.after(), new Slice(content, 0, 0));
            tr.setSelection(selectionNear(tr.doc, start + prefix.size + frag.size - 1));
        });
        const tr = literal
            ? tryBuild(state, (tr) => insertTextIn(tr, state, text))
            : ((isMarkdown || looksLikeMarkdown(text)) ? markdown() : null) || plain();
        if (tr?.docChanged) dispatch(tr);
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

    function finishComposition() {
        const active = document.activeElement;
        if (active && view.dom.contains(active)) active.blur();
        view.onCompositionEnd();
    }

    return {
        /** Runs a selection-toolbar command by name (toolbar.js).
         *  `arg` reaches commands that take one (setCodeLanguage). */
        exec(name, arg) {
            const command = commands[name];
            if (!command) return false;
            if (view.composing) finishComposition();
            view.onSelectionChange();
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
            finishComposition();
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
