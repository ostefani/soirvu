// Our own editing commands (substitute for prosemirror-commands
// and prosemirror-schema-list).
//
// A command is (state, dispatch?) => boolean: it reports whether
// it can run, and when dispatch is given it builds a transaction
// on a scratch copy first — a command whose steps fail anywhere
// dispatches nothing.

import { Fragment } from "prosemirror-model";
import { closedSlice, inlineSlice } from "./transform.js";
import { NodeSelection, TextSelection, cursorMarks, selectionNear } from "./state.js";

// ---------------------------------------------------------------------------
// Helpers shared by commands and rules

/** Runs build(tr) on a scratch transaction; null if any step fails. */
export function tryBuild(state, build) {
    const tr = state.tr();
    try {
        build(tr);
        tr.doc.check();
    } catch {
        return null;
    }
    return tr;
}

function finish(state, dispatch, build) {
    const tr = tryBuild(state, build);
    if (!tr) return false;
    if (dispatch) dispatch(tr);
    return true;
}

export function chainCommands(...commands) {
    return (state, dispatch) => {
        for (const cmd of commands) {
            if (cmd(state, dispatch)) return true;
        }
        return false;
    };
}

/** Deletes the current selection, if any. Returns the insert point. */
export function deleteSelectionIn(tr, state) {
    const sel = state.selection;
    const doc = tr.doc;
    if (sel.type === "node") {
        const node = sel.node(doc);
        tr.delete(sel.from, sel.from + node.nodeSize);
        return sel.from;
    }
    if (!sel.empty) tr.delete(sel.from, sel.to);
    return sel.from;
}

/** Inserts text at the selection (replacing it), with typing marks. */
export function insertTextIn(tr, state, text) {
    const pos = deleteSelectionIn(tr, state);
    const marks = cursorMarks(state);
    const node = text ? state.schema.text(text, marks) : null;
    if (node) {
        tr.replaceRange(pos, pos, inlineSlice(Fragment.from(node)));
    }
    tr.setSelection(new TextSelection(pos + text.length));
    if (state.storedMarks) tr.setStoredMarks(state.storedMarks);
    return pos + text.length;
}

export function deleteSelection(state, dispatch) {
    if (state.selection.empty) return false;
    return finish(state, dispatch, (tr) => {
        const pos = deleteSelectionIn(tr, state);
        tr.setSelection(selectionNear(tr.doc, pos));
    });
}

/** Deletes one code point before (dir -1) or after (dir +1) a collapsed cursor. */
export function deleteChar(dir) {
    return (state, dispatch) => {
        const sel = state.selection;
        if (sel.type !== "text" || !sel.empty) return false;
        const doc = state.doc;
        const $pos = doc.resolve(sel.from);
        if (!$pos.parent.isTextblock) return false;
        const text = $pos.parent.textContent;
        const offset = $pos.parentOffset;
        if (dir < 0 && offset === 0) return false;
        if (dir > 0 && offset === text.length) return false;
        let from = sel.from;
        let to = sel.from;
        if (dir < 0) {
            const before = text.slice(0, offset);
            const ch = [...before].pop();
            from -= ch ? ch.length : 1;
        } else {
            const after = text.slice(offset);
            const ch = [...after][0];
            to += ch ? ch.length : 1;
        }
        return finish(state, dispatch, (tr) => {
            tr.delete(from, to);
            tr.setSelection(new TextSelection(from));
        });
    };
}

/** Deletes by word before/after the cursor, within the current block. */
export function deleteWord(dir) {
    return (state, dispatch) => {
        const sel = state.selection;
        if (sel.type !== "text" || !sel.empty) return false;
        const doc = state.doc;
        const $pos = doc.resolve(sel.from);
        if (!$pos.parent.isTextblock) return false;
        const text = $pos.parent.textContent;
        const offset = $pos.parentOffset;
        let from = sel.from;
        let to = sel.from;
        if (dir < 0) {
            const m = text.slice(0, offset).match(/\s*\S+[\s]*$|\s+$/);
            if (!m) return false;
            from -= m[0].length;
        } else {
            const m = text.slice(offset).match(/^\s*\S+|^\s+/);
            if (!m) return false;
            to += m[0].length;
        }
        if (from === to) return false;
        return finish(state, dispatch, (tr) => {
            tr.delete(from, to);
            tr.setSelection(new TextSelection(from));
        });
    };
}

// ---------------------------------------------------------------------------
// Splitting blocks (Enter)

/** Splits the textblock at the selection. typeAfter follows the
 *  classic rule: at the end of a non-paragraph block, the new block
 *  is a paragraph; in the middle, it keeps the block's type. */
export function splitBlock(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    if (!$from.parent.isTextblock) return false;
    return finish(state, dispatch, (tr) => {
        const pos = deleteSelectionIn(tr, state);
        const $p = tr.doc.resolve(pos);
        const block = $p.parent;
        const blockStart = $p.start();
        const blockEnd = $p.end();
        const atEnd = pos === blockEnd;
        const schema = state.schema;
        const typeAfter =
            atEnd && block.type !== schema.nodes.paragraph
                ? schema.nodes.paragraph
                : block.type;
        const attrsAfter = typeAfter === block.type ? block.attrs : typeAfter.spec.attrs ? defaultAttrs(typeAfter) : {};
        const before = block.type.create(block.attrs, block.content.cut(0, $p.parentOffset));
        const after = typeAfter.create(
            typeAfter === block.type ? block.attrs : attrsAfter,
            block.content.cut($p.parentOffset),
        );
        tr.replaceWith(blockStart - 1, blockEnd + 1, [before, after]);
        tr.setSelection(new TextSelection(blockStart + before.nodeSize));
    });
}

function defaultAttrs(type) {
    const out = {};
    for (const [k, v] of Object.entries(type.spec.attrs || {})) out[k] = v.default;
    return out;
}

/** Enter inside a code block inserts a newline; Enter on the empty
 *  last line leaves the block. */
export function newlineInCode(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const $from = state.doc.resolve(sel.from);
    if (!$from.parent.type.spec.code) return false;
    const block = $from.parent;
    const text = block.textContent;
    const offset = $from.parentOffset;
    const atVeryEnd = sel.empty && offset === text.length;
    if (atVeryEnd && (text === "" || text.endsWith("\n"))) {
        // Leave the block: drop the trailing newline, split into a paragraph.
        return finish(state, dispatch, (tr) => {
            const pos = deleteSelectionIn(tr, state);
            const $p = tr.doc.resolve(pos);
            const b = $p.parent;
            const start = $p.start();
            const end = $p.end();
            const trimmed = b.textContent.replace(/\n$/, "");
            const codeNode = b.type.create(b.attrs, trimmed ? state.schema.text(trimmed) : null);
            const para = state.schema.nodes.paragraph.create();
            tr.replaceWith(start - 1, end + 1, [codeNode, para]);
            tr.setSelection(new TextSelection(start + codeNode.nodeSize));
        });
    }
    return finish(state, dispatch, (tr) => {
        insertTextIn(tr, state, "\n");
    });
}

/** If the cursor is in an empty non-paragraph block, lift it out of
 *  its wrapper (or turn it into a paragraph at the top level). */
export function liftEmptyBlock(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text" || !sel.empty) return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    const block = $from.parent;
    if (!block.isTextblock || block.content.size > 0) return false;
    const schema = state.schema;
    if ($from.depth <= 1) {
        if (block.type === schema.nodes.paragraph) return false;
        return finish(state, dispatch, (tr) => {
            const start = $from.start() - 1;
            tr.replaceWith(start, start + block.nodeSize, [schema.nodes.paragraph.create()]);
            tr.setSelection(new TextSelection(start + 1));
        });
    }
    if ($from.node($from.depth - 1).type.name === "list_item") {
        return liftChild(state, dispatch, $from.depth - 2, $from.index($from.depth - 2));
    }
    return liftChild(state, dispatch, $from.depth - 1, $from.index($from.depth - 1));
}

/** Creates a paragraph next to the selection when the cursor sits
 *  at the edge of a non-text block (e.g. after a horizontal rule). */
export function createParagraphNear(state, dispatch) {
    const sel = state.selection;
    if (sel.type === "node") {
        const para = state.schema.nodes.paragraph;
        const pos = sel.from + sel.node(state.doc).nodeSize;
        return finish(state, dispatch, (tr) => {
            tr.insert(pos, [para.create()]);
            tr.setSelection(new TextSelection(pos + 1));
        });
    }
    if (sel.type !== "text" || !sel.empty) return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    if ($from.parent.isTextblock && $from.parent.content.size > 0) return false;
    const para = state.schema.nodes.paragraph;
    let pos = null;
    if ($from.parentOffset === 0) pos = $from.before($from.depth || 1);
    else pos = $from.after($from.depth || 1);
    if (pos === null || pos === undefined) return false;
    return finish(state, dispatch, (tr) => {
        tr.insert(pos, [para.create()]);
        tr.setSelection(new TextSelection(pos + 1));
    });
}

// ---------------------------------------------------------------------------
// Lifting (shared primitive)

/** Lifts the child at (parentDepth-relative position) out of its
 *  parent: the parent keeps its other children (split in two if
 *  the child is in the middle), and the child's own content — or
 *  the child itself for leaf blocks — lands between them.
 *  Positions are given as the resolved depth of the PARENT node
 *  and the child's index within it, both in the current doc. */
export function liftChild(state, dispatch, parentDepth, childIndex) {
    const doc = state.doc;
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const $from = doc.resolve(sel.from);
    if (parentDepth < 1 || parentDepth > $from.depth) return false;
    const parent = $from.node(parentDepth);
    const parentStart = $from.before(parentDepth); // position before parent
    const child = parent.child(childIndex);
    const moved =
        child.type.name === "list_item"
            ? child.content.content // the item's blocks move out
            : [child];
    const beforeKids = [];
    const afterKids = [];
    for (let i = 0; i < parent.childCount; i++) {
        if (i < childIndex) beforeKids.push(parent.child(i));
        else if (i > childIndex) afterKids.push(parent.child(i));
    }
    const replacement = [];
    if (beforeKids.length) replacement.push(parent.type.create(parent.attrs, Fragment.fromArray(beforeKids)));
    for (const n of moved) replacement.push(n);
    if (afterKids.length) replacement.push(parent.type.create(parent.attrs, Fragment.fromArray(afterKids)));
    // Where does the cursor go? To the start of the moved content.
    let cursor = parentStart;
    if (beforeKids.length) cursor += replacement[0].nodeSize;
    cursor += 1; // step inside the moved block/item content
    return finish(state, dispatch, (tr) => {
        tr.replaceWith(parentStart, parentStart + parent.nodeSize, replacement);
        tr.setSelection(selectionNear(tr.doc, cursor));
    });
}

export function liftListItem(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    let itemDepth = -1;
    for (let d = $from.depth; d > 0; d--) {
        if ($from.node(d).type.name === "list_item") {
            itemDepth = d;
            break;
        }
    }
    if (itemDepth < 1) return false;
    const listDepth = itemDepth - 1;
    const list = $from.node(listDepth);
    const itemIndex = $from.index(listDepth);
    const parent = $from.node(listDepth - 1);
    if (!parent || parent.type.name !== "list_item") {
        // The list sits at the top level (or in a quote): the item's
        // content simply leaves the list.
        return liftChild(state, dispatch, listDepth, itemIndex);
    }
    // A nested list: the item moves into the grandparent list,
    // right after the item that contains its list.
    const grandDepth = listDepth - 2;
    const grandList = $from.node(grandDepth);
    const containerIndex = $from.index(grandDepth);
    const container = parent;
    const item = $from.node(itemDepth);
    const remainingItems = [];
    for (let i = 0; i < list.childCount; i++) {
        if (i !== itemIndex) remainingItems.push(list.child(i));
    }
    const containerKids = [];
    container.content.forEach((child) => {
        if (child === list) {
            if (remainingItems.length) {
                containerKids.push(list.type.create(list.attrs, Fragment.fromArray(remainingItems)));
            }
        } else {
            containerKids.push(child);
        }
    });
    const newContainer = container.type.create(container.attrs, Fragment.fromArray(containerKids));
    const grandKids = [];
    let prefixSize = 0;
    for (let i = 0; i < grandList.childCount; i++) {
        if (i === containerIndex) {
            grandKids.push(newContainer);
            prefixSize += newContainer.nodeSize;
            grandKids.push(item);
        } else {
            grandKids.push(grandList.child(i));
            if (i < containerIndex) prefixSize += grandList.child(i).nodeSize;
        }
    }
    const grandStart = $from.before(grandDepth);
    const cursor = grandStart + prefixSize + 2 + $from.parentOffset;
    return finish(state, dispatch, (tr) => {
        tr.replaceWith(grandStart, grandStart + grandList.nodeSize, [
            grandList.type.create(grandList.attrs, Fragment.fromArray(grandKids)),
        ]);
        tr.setSelection(selectionNear(tr.doc, cursor));
    });
}

// ---------------------------------------------------------------------------
// List Enter / Tab

export function splitListItem(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    let itemDepth = -1;
    for (let d = $from.depth; d > 0; d--) {
        if ($from.node(d).type.name === "list_item") {
            itemDepth = d;
            break;
        }
    }
    if (itemDepth < 0) return false;
    const item = $from.node(itemDepth);
    if (item.content.size === 0 || isEmptyItem(item)) {
        // Enter on an empty item ends the list at this point.
        return liftListItem(state, dispatch);
    }
    return finish(state, dispatch, (tr) => {
        const pos = deleteSelectionIn(tr, state);
        const $p = tr.doc.resolve(pos);
        // Re-find the item around the (possibly moved) cursor.
        let d = $p.depth;
        while (d > 0 && $p.node(d).type.name !== "list_item") d--;
        const it = $p.node(d);
        const itemStart = $p.before(d);
        // Split the item's content at the cursor: walk down to the textblock.
        const inner = $p.parent;
        const innerStart = $p.start();
        const beforeBlocks = [];
        const afterBlocks = [];
        let seenInner = false;
        it.content.forEach((child) => {
            if (child === inner) {
                seenInner = true;
                const off = $p.parentOffset;
                const b = child.type.create(child.attrs, child.content.cut(0, off));
                const a = child.type.create(child.attrs, child.content.cut(off));
                beforeBlocks.push(b);
                afterBlocks.push(a);
            } else if (!seenInner) {
                beforeBlocks.push(child);
            } else {
                afterBlocks.push(child);
            }
        });
        // The next item starts fresh: a task splits into an
        // unchecked task (the list stays a task list), anything
        // else keeps its attrs as they were.
        const itemAttrs = { ...it.attrs };
        if (itemAttrs.checked !== null && itemAttrs.checked !== undefined) {
            itemAttrs.checked = false;
        }
        const item1 = it.type.create(it.attrs, Fragment.fromArray(beforeBlocks));
        const item2 = it.type.create(itemAttrs, Fragment.fromArray(afterBlocks));
        tr.replaceWith(itemStart, itemStart + it.nodeSize, [item1, item2]);
        tr.setSelection(new TextSelection(itemStart + item1.nodeSize + 2));
    });
}

function isEmptyItem(item) {
    return item.childCount === 1 && item.firstChild.isTextblock && item.firstChild.content.size === 0;
}

export function sinkListItem(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text") return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    let itemDepth = -1;
    for (let d = $from.depth; d > 0; d--) {
        if ($from.node(d).type.name === "list_item") {
            itemDepth = d;
            break;
        }
    }
    if (itemDepth < 1) return false;
    const listDepth = itemDepth - 1;
    const index = $from.index(listDepth);
    if (index === 0) return false; // no previous item to nest under
    const list = $from.node(listDepth);
    const item = $from.node(itemDepth);
    const listStart = $from.before(listDepth);
    const prevItem = list.child(index - 1);
    // The previous item gains a nested list holding this item.
    const newPrevContent = [];
    prevItem.content.forEach((n) => newPrevContent.push(n));
    const lastChild = newPrevContent[newPrevContent.length - 1];
    if (lastChild && (lastChild.type === list.type)) {
        newPrevContent[newPrevContent.length - 1] = lastChild.type.create(
            lastChild.attrs,
            Fragment.fromArray([...lastChild.content.content, item]),
        );
    } else {
        newPrevContent.push(list.type.create(list.attrs, Fragment.fromArray([item])));
    }
    const newPrev = prevItem.type.create(prevItem.attrs, Fragment.fromArray(newPrevContent));
    const remaining = [];
    for (let i = 0; i < list.childCount; i++) {
        if (i === index - 1) remaining.push(newPrev);
        else if (i !== index) remaining.push(list.child(i));
    }
    const cursor = sel.from + 2; // the item moved one level deeper
    return finish(state, dispatch, (tr) => {
        tr.replaceWith(listStart, listStart + list.nodeSize, [
            list.type.create(list.attrs, Fragment.fromArray(remaining)),
        ]);
        tr.setSelection(selectionNear(tr.doc, cursor));
    });
}

// ---------------------------------------------------------------------------
// Joining (Backspace / Delete at block edges)

function joinAt(state, dispatch, pos) {
    const doc = state.doc;
    const $pos = doc.resolve(pos);
    const before = $pos.nodeBefore;
    const after = $pos.nodeAfter;
    if (!before || !after) return false;
    if (!(before.isTextblock && after.isTextblock) && before.type !== after.type) return false;
    const start = pos - before.nodeSize;
    const merged = before.type.create(
        before.attrs,
        before.content.append(after.content),
    );
    return finish(state, dispatch, (tr) => {
        tr.replaceWith(start, start + before.nodeSize + after.nodeSize, [merged]);
        tr.setSelection(new TextSelection(start + 1 + before.content.size));
    });
}

export function joinBackward(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text" || !sel.empty) return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    if ($from.parentOffset > 0) return false;
    // Find the deepest level at which the block has a previous sibling.
    for (let d = $from.depth; d > 0; d--) {
        if ($from.index(d - 1) > 0) {
            return joinAt(state, dispatch, $from.before(d));
        }
    }
    // First child everywhere: leave the innermost wrapper — the
    // item, if this is a list, or the enclosing block otherwise.
    for (let d = $from.depth; d > 0; d--) {
        if ($from.node(d).type.name === "list_item") return liftListItem(state, dispatch);
    }
    if ($from.depth >= 2) {
        return liftChild(state, dispatch, 1, $from.index(1));
    }
    return false;
}

export function joinForward(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text" || !sel.empty) return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    if ($from.parentOffset < $from.parent.content.size) return false;
    for (let d = $from.depth; d > 0; d--) {
        if ($from.index(d - 1) + 1 < $from.node(d - 1).childCount) {
            return joinAt(state, dispatch, $from.after(d));
        }
    }
    return false;
}

export function selectNodeBackward(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text" || !sel.empty) return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    if ($from.parentOffset > 0) return false;
    const before = $from.nodeBefore;
    if (before && !before.isText) {
        const pos = sel.from - before.nodeSize;
        return finish(state, dispatch, (tr) => {
            tr.setSelection(new NodeSelection(pos));
        });
    }
    return false;
}

export function selectNodeForward(state, dispatch) {
    const sel = state.selection;
    if (sel.type !== "text" || !sel.empty) return false;
    const doc = state.doc;
    const $from = doc.resolve(sel.from);
    if ($from.parentOffset < $from.parent.content.size) return false;
    const after = $from.nodeAfter;
    if (after && !after.isText) {
        return finish(state, dispatch, (tr) => {
            tr.setSelection(new NodeSelection(sel.from));
        });
    }
    return false;
}

// ---------------------------------------------------------------------------
// Marks

export function markActive(state, markType) {
    const sel = state.selection;
    const doc = state.doc;
    if (sel.type !== "text") return false;
    if (sel.empty) return cursorMarks(state).some((m) => m.type === markType);
    let active = true;
    let seen = false;
    doc.nodesBetween(sel.from, sel.to, (node) => {
        if (node.isText) {
            seen = true;
            if (!markType.isInSet(node.marks)) active = false;
        }
        return true;
    });
    return seen && active;
}

/** Applies or removes a mark over one textblock segment of the range. */
function markRangeInBlock(tr, blockStart, contentStart, from, to, markType, add) {
    const doc = tr.doc;
    const $from = doc.resolve(contentStart);
    const block = $from.parent;
    const newChildren = [];
    let offset = 0;
    block.content.forEach((child) => {
        const childStart = contentStart + offset;
        const childEnd = childStart + child.nodeSize;
        offset += child.nodeSize;
        if (!child.isText || childEnd <= from || childStart >= to) {
            newChildren.push(child);
            return;
        }
        const len = child.text.length;
        const cutFrom = Math.max(0, from - childStart);
        const cutTo = Math.min(len, to - childStart);
        if (cutFrom > 0) newChildren.push(child.cut(0, cutFrom));
        const mid = child.cut(cutFrom, cutTo);
        newChildren.push(
            add
                ? mid.mark(markType.create().addToSet(mid.marks))
                : mid.mark(markType.removeFromSet(mid.marks)),
        );
        if (cutTo < len) newChildren.push(child.cut(cutTo));
    });
    const newBlock = block.type.create(block.attrs, Fragment.fromArray(newChildren), block.marks);
    tr.replaceWith(blockStart, blockStart + block.nodeSize, [newBlock]);
}

export function toggleMark(markType) {
    return (state, dispatch) => {
        const sel = state.selection;
        if (sel.type !== "text") return false;
        const doc = state.doc;
        if (sel.empty) {
            return finish(state, dispatch, (tr) => {
                const marks = cursorMarks(state);
                const has = markType.isInSet(marks);
                tr.setStoredMarks(
                    has ? markType.removeFromSet(marks) : markType.create().addToSet(marks),
                );
            });
        }
        const add = !markActive(state, markType);
        return finish(state, dispatch, (tr) => {
            // One segment per textblock, applied back to front.
            const segments = [];
            doc.nodesBetween(sel.from, sel.to, (node, pos) => {
                if (node.isTextblock) {
                    const start = Math.max(sel.from, pos + 1);
                    const end = Math.min(sel.to, pos + node.nodeSize - 1);
                    if (start < end) segments.push({ blockStart: pos, contentStart: pos + 1, from: start, to: end });
                    return false;
                }
                return true;
            });
            for (let i = segments.length - 1; i >= 0; i--) {
                const s = segments[i];
                markRangeInBlock(tr, s.blockStart, s.contentStart, s.from, s.to, markType, add);
            }
            tr.setSelection(new TextSelection(sel.anchor, sel.head));
            tr.setStoredMarks(null);
        });
    };
}

/** Sets the textblock type for every textblock touching the selection. */
export function setBlockType(nodeType, attrs = {}) {
    return (state, dispatch) => {
        const sel = state.selection;
        if (sel.type !== "text") return false;
        const doc = state.doc;
        return finish(state, dispatch, (tr) => {
            const blocks = [];
            doc.nodesBetween(sel.from, sel.to, (node, pos) => {
                if (node.isTextblock) {
                    blocks.push({ node, pos });
                    return false;
                }
                return true;
            });
            for (let i = blocks.length - 1; i >= 0; i--) {
                const { node, pos } = blocks[i];
                tr.replaceWith(pos, pos + node.nodeSize, [
                    nodeType.create(attrs, node.content, node.marks),
                ]);
            }
            tr.setSelection(selectionNear(tr.doc, sel.from));
        });
    };
}

/** Wraps the block range around the selection in the given chain
 *  (outermost first). If the previous sibling is a container of the
 *  outermost type, the range joins it instead (list continuation). */
export function wrapBlocks(wrappers) {
    return (state, dispatch) => {
        const sel = state.selection;
        if (sel.type !== "text") return false;
        const doc = state.doc;
        const $from = doc.resolve(sel.from);
        const $to = doc.resolve(sel.to);
        const range = $from.blockRange($to);
        if (!range) return false;
        return finish(state, dispatch, (tr) => {
            const start = range.start;
            const end = range.end;
            const content = tr.doc.slice(start, end).content;
            let built = content;
            for (let i = wrappers.length - 1; i >= 0; i--) {
                const w = wrappers[i];
                built = Fragment.from(w.type.create(w.attrs || {}, built));
            }
            const outer = built.firstChild;
            // Join the previous sibling when it is the same container.
            const $start = tr.doc.resolve(start);
            const parentDepth = range.depth;
            const idx = $start.index(parentDepth);
            if (idx > 0) {
                const parent = $start.node(parentDepth);
                const prev = parent.child(idx - 1);
                if (prev.type === outer.type && wrappers.length >= 1) {
                    const prevStart = start - prev.nodeSize;
                    const joined = prev.type.create(prev.attrs, prev.content.append(outer.content));
                    const cursor =
                        prevStart + 1 + prev.content.size + (wrappers.length - 1) + (sel.from - start);
                    tr.replaceWith(prevStart, end, [joined]);
                    tr.setSelection(selectionNear(tr.doc, cursor));
                    return;
                }
            }
            tr.replaceWith(start, end, [outer]);
            tr.setSelection(selectionNear(tr.doc, sel.from + wrappers.length));
        });
    };
}

/** Insert a hard break at the selection (Shift-Enter). */
export function insertHardBreak(state, dispatch) {
    const hardBreak = state.schema.nodes.hard_break;
    if (!hardBreak) return false;
    return finish(state, dispatch, (tr) => {
        const pos = deleteSelectionIn(tr, state);
        tr.replaceRange(pos, pos, inlineSlice(Fragment.from(hardBreak.create())));
        tr.setSelection(new TextSelection(pos + 1));
    });
}
