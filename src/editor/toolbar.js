// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// The selection toolbar's brain (§6.5): what the current
// selection looks like (selectionInfo) and the commands its
// buttons run (toolbarCommands). Both are pure functions of the
// editor state — the Svelte component only renders and forwards
// clicks, so all of this is testable without a DOM.

import { Fragment } from "prosemirror-model";
import {
    liftChild,
    liftListItem,
    markActive,
    setBlockType,
    toggleMark,
    wrapBlocks,
} from "./commands.js";
import { TextSelection, selectionNear } from "./state.js";
import {
    addColumnCommand,
    addRowCommand,
    deleteColumnCommand,
    deleteRowCommand,
    insertTableCommand,
    tableContext,
} from "./table.js";

/** A plain description of the selection, for rendering the
 *  toolbar's active states. `block` is the textblock at the
 *  cursor; `list` is the kind of list the cursor sits in, where
 *  a task is a bullet item whose checked attr is set. */
export function selectionInfo(state) {
    const sel = state.selection;
    const info = {
        from: sel.from ?? 0,
        to: sel.to ?? 0,
        empty: sel.type !== "text" || sel.empty,
        strong: false,
        em: false,
        strike: false,
        code: false,
        block: "other",
        headingLevel: null,
        quote: false,
        list: null,
        inTable: false,
        codeLang: null,
    };
    if (sel.type !== "text") return info;
    const { marks, nodes } = state.schema;
    info.strong = markActive(state, marks.strong);
    info.em = markActive(state, marks.em);
    info.strike = markActive(state, marks.strike);
    info.code = markActive(state, marks.code);
    const $from = state.doc.resolve(sel.from);
    const parent = $from.parent;
    if (parent.type === nodes.heading) {
        info.block = "heading";
        info.headingLevel = parent.attrs.level;
    } else if (parent.type === nodes.code_block) {
        info.block = "code_block";
        info.codeLang = parent.attrs.params || "";
    } else if (parent.type === nodes.paragraph) {
        info.block = "paragraph";
    }
    info.inTable = Boolean(tableContext(state));
    for (let d = 1; d <= $from.depth; d++) {
        const node = $from.node(d);
        if (node.type === nodes.blockquote) info.quote = true;
        if (node.type === nodes.list_item) {
            const list = $from.node(d - 1);
            const checked = node.attrs.checked;
            info.list =
                checked !== null && checked !== undefined
                    ? "task"
                    : list.type === nodes.ordered_list
                      ? "ordered"
                      : "bullet";
        }
    }
    return info;
}

/** Rebuilds the nearest ancestor matching `pred` with `build`
 *  (list-type conversion, the checked attr flip). Sizes are
 *  preserved by construction, so the selection survives. */
function replaceAncestor(state, dispatch, pred, build) {
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const $from = state.doc.resolve(sel.from);
    for (let d = 1; d <= $from.depth; d++) {
        const node = $from.node(d);
        if (!pred(node)) continue;
        const pos = $from.before(d);
        const tr = state.tr();
        tr.replaceWith(pos, pos + node.nodeSize, [build(node)]);
        tr.setSelection(selectionNear(tr.doc, Math.min(sel.from, tr.doc.content.size)));
        if (dispatch) dispatch(tr);
        return true;
    }
    return false;
}

/** The toolbar's commands, by button name. Each is a normal
 *  (state, dispatch) command; toggles decide from selectionInfo,
 *  so the buttons and their active states can never disagree
 *  about what a click will do. */
export function toolbarCommands(schema) {
    const { marks, nodes } = schema;
    const toParagraph = (state, dispatch) => setBlockType(nodes.paragraph)(state, dispatch);
    const heading = (level) => (state, dispatch) =>
        selectionInfo(state).headingLevel === level
            ? toParagraph(state, dispatch)
            : setBlockType(nodes.heading, { level })(state, dispatch);

    function listCommand(kind) {
        const listType = kind === "bullet" ? nodes.bullet_list : nodes.ordered_list;
        const attrs = kind === "bullet" ? { tight: true } : { order: 1, tight: true };
        return (state, dispatch) => {
            const info = selectionInfo(state);
            if (info.list === kind) return liftListItem(state, dispatch);
            if (info.list) {
                // Convert: same items under the other list type; a
                // task list gives up its checkboxes on the way.
                return replaceAncestor(
                    state,
                    dispatch,
                    (n) => n.type === nodes.bullet_list || n.type === nodes.ordered_list,
                    (list) => {
                        const items = [];
                        list.content.forEach((item) => {
                            items.push(
                                info.list === "task"
                                    ? nodes.list_item.create(
                                          { ...item.attrs, checked: null },
                                          item.content,
                                          item.marks,
                                      )
                                    : item,
                            );
                        });
                        return listType.create(attrs, Fragment.fromArray(items));
                    },
                );
            }
            return wrapBlocks([
                { type: listType, attrs },
                { type: nodes.list_item },
            ])(state, dispatch);
        };
    }

    return {
        strong: toggleMark(marks.strong),
        em: toggleMark(marks.em),
        strike: toggleMark(marks.strike),
        code: toggleMark(marks.code),
        paragraph: toParagraph,
        heading1: heading(1),
        heading2: heading(2),
        heading3: heading(3),
        codeBlock: (state, dispatch) =>
            selectionInfo(state).block === "code_block"
                ? toParagraph(state, dispatch)
                : setBlockType(nodes.code_block)(state, dispatch),
        quote: (state, dispatch) => {
            if (!selectionInfo(state).quote) {
                return wrapBlocks([{ type: nodes.blockquote }])(state, dispatch);
            }
            // Lift the current block out of the quote.
            const $from = state.doc.resolve(state.selection.from);
            for (let d = 1; d <= $from.depth; d++) {
                if ($from.node(d).type === nodes.blockquote) {
                    return liftChild(state, dispatch, d, $from.index(d));
                }
            }
            return false;
        },
        setCodeLanguage: (state, dispatch, lang) => setCodeLanguage(state, dispatch, lang),
        insertTable: insertTableCommand,
        addRow: addRowCommand,
        addColumn: addColumnCommand,
        deleteRow: deleteRowCommand,
        deleteColumn: deleteColumnCommand,
        bulletList: listCommand("bullet"),
        orderedList: listCommand("ordered"),
        taskList: (state, dispatch) => {
            const info = selectionInfo(state);
            if (info.list) {
                // Flip the item's checked attr: unset makes it a
                // task, false/true makes it a plain bullet again.
                return replaceAncestor(
                    state,
                    dispatch,
                    (n) => n.type === nodes.list_item,
                    (item) =>
                        nodes.list_item.create(
                            {
                                ...item.attrs,
                                checked:
                                    item.attrs.checked === null ||
                                    item.attrs.checked === undefined
                                        ? false
                                        : null,
                            },
                            item.content,
                            item.marks,
                        ),
                );
            }
            return wrapBlocks([
                { type: nodes.bullet_list, attrs: { tight: true } },
                { type: nodes.list_item, attrs: { checked: false } },
            ])(state, dispatch);
        },
    };
}

/** Sets the language (the fence info string) of the code
 *  block at the cursor — how a typed block gains highlighting
 *  after the fact, and how a pasted one changes language.
 *  The block keeps its content and the cursor its place. */
export function setCodeLanguage(state, dispatch, lang) {
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const $from = state.doc.resolve(sel.from);
    const block = $from.parent;
    if (block.type.name !== "code_block") return false;
    const pos = $from.before();
    const tr = state.tr();
    tr.replaceWith(pos, pos + block.nodeSize, [
        block.type.create({ ...block.attrs, params: (lang || "").trim() }, block.content, block.marks),
    ]);
    tr.setSelection(new TextSelection(sel.from));
    if (dispatch) dispatch(tr);
    return true;
}

/** Flips the checked state of the task item at `itemPos` (a
 *  document position just before the list_item node). Used by
 *  the view when the item's checkbox is clicked. */
export function toggleTaskAt(state, dispatch, itemPos) {
    const item = state.doc.nodeAt(itemPos);
    if (!item || item.type.name !== "list_item") return false;
    if (item.attrs.checked === null || item.attrs.checked === undefined) return false;
    const tr = state.tr();
    tr.replaceWith(itemPos, itemPos + item.nodeSize, [
        item.type.create({ ...item.attrs, checked: !item.attrs.checked }, item.content, item.marks),
    ]);
    if (dispatch) dispatch(tr);
    return true;
}
