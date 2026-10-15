// The editor component (§6.5): createEditor mounts our own view
// over our own state, commands, rules and history. The only
// borrowed pieces are the document model and the Markdown layer
// (prosemirror-model / prosemirror-markdown / markdown-it, in
// schema.js and markdown.js). Svelte talks to the editor through
// the returned object; the editor owns the document in between.
//
// The baseline rule lives in the wiring: onChange fires only for
// transactions that change the document, so opening an entry and
// closing it untouched saves nothing and rewrites nothing.

import { Fragment, Slice } from "prosemirror-model";
import { EditorState, TextSelection, selectionNear } from "./state.js";
import { EditorView } from "./view.js";
import { historyPlugin } from "./history.js";
import { inputRulesPlugin, keymapPlugin } from "./rules.js";
import { deleteSelectionIn, insertTextIn } from "./commands.js";
import { selectionInfo, toolbarCommands, toggleTaskAt } from "./toolbar.js";
import { schema } from "./schema.js";
import { parseMarkdown, serializeMarkdown } from "./markdown.js";

export { schema, parseMarkdown, serializeMarkdown };

function looksLikeMarkdown(text) {
    return /(^|\n)\s{0,3}(#{1,6}\s|>|```|~~~|- |\d+\.\s)|(\*\*|__)[^*\n]+\1|\[[^\]\n]+\]\(https?:[^)\s]+\)/.test(
        text,
    );
}

/** Paste policy (§6.5): plain text goes in as text (newlines as
 *  hard breaks, blank lines as paragraph splits); text that looks
 *  like Markdown is parsed as Markdown. Foreign HTML never enters
 *  the schema — only the clipboard's text/plain is ever read. */
export function makePasteHandler() {
    return (text, state, dispatch) => {
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
        if (looksLikeMarkdown(text)) {
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
        text.split(/\n{2,}/).forEach((para, i) => {
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
                    insertTextIn(tr, current, line);
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
 * @param {(href: string) => void} [options.onOpenLink] A link was
 *   clicked; the app routes https/mailto through open_link.
 * @param {() => void} [options.onSelectionChange] Called after
 *   every transaction, doc or selection, so floating UI (the
 *   selection toolbar) can re-render from query().
 */
export function createEditor(el, options = {}) {
    const { markdown = "", onChange = () => {}, onOpenLink = null, onSelectionChange = null, onAttach = null, onAttachmentView = null } = options;
    let readonly = false;

    const plugins = [inputRulesPlugin(), keymapPlugin(schema), historyPlugin()];

    const view = new EditorView(el, {
        state: EditorState.create({ doc: parseMarkdown(markdown), plugins }),
        dispatchTransaction(tr) {
            const next = view.state.apply(tr);
            view.updateState(next);
            if (tr.docChanged) onChange(serializeMarkdown(next.doc));
            if (onSelectionChange) onSelectionChange();
            hydrateAttachments();
        },
        editable: () => !readonly,
        onTaskToggle: (itemPos) => {
            toggleTaskAt(view.state, (tr) => view.dispatch(tr), itemPos);
        },
        handleClickOn: (href) => {
            if (onOpenLink && href) onOpenLink(href);
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
        /** Replaces the document (switching entries). History resets. */
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
