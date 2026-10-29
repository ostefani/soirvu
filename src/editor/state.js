// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Our own editor state (substitute for prosemirror-state).
//
// A state is immutable: a document, a selection, the marks that
// typing would continue (stored marks), and one state field per
// plugin. Applying a transaction produces the next state; plugin
// fields update through their own apply functions.

import { Transaction } from "./transform.js";

export class TextSelection {
    constructor(anchor, head = anchor) {
        this.type = "text";
        this.anchor = anchor;
        this.head = head;
    }

    get empty() {
        return this.anchor === this.head;
    }

    get from() {
        return Math.min(this.anchor, this.head);
    }

    get to() {
        return Math.max(this.anchor, this.head);
    }

    $from(doc) {
        return doc.resolve(this.from);
    }

    $to(doc) {
        return doc.resolve(this.to);
    }

    $anchor(doc) {
        return doc.resolve(this.anchor);
    }

    $head(doc) {
        return doc.resolve(this.head);
    }

    eq(other) {
        return other && other.type === "text" && other.anchor === this.anchor && other.head === this.head;
    }
}

export class NodeSelection {
    constructor(anchor) {
        this.type = "node";
        this.anchor = anchor;
        this.head = anchor;
    }

    get empty() {
        return false;
    }

    get from() {
        return this.anchor;
    }

    to(doc) {
        return this.anchor + doc.nodeAt(this.anchor).nodeSize;
    }

    node(doc) {
        return doc.nodeAt(this.anchor);
    }

    eq(other) {
        return other && other.type === "node" && other.anchor === this.anchor;
    }
}

export function isTextSelection(sel) {
    return sel && sel.type === "text";
}

/** The nearest valid text-cursor position to `pos` in `doc`. */
export function selectionNear(doc, pos, bias = 1) {
    pos = Math.max(0, Math.min(pos, doc.content.size));
    const $pos = doc.resolve(pos);
    if ($pos.parent.isTextblock) {
        // Positions inside an inline atom are not cursor positions.
        const before = $pos.nodeBefore;
        const after = $pos.nodeAfter;
        if (after && !after.isText && after.isInline) return new TextSelection(pos + after.nodeSize);
        if (before && !before.isText && before.isInline && pos === $pos.start() + $pos.parentOffset) {
            return new TextSelection(pos);
        }
        return new TextSelection(pos);
    }
    // Not in a textblock: walk to the nearest textblock position.
    let best = null;
    doc.descendants((node, p) => {
        if (node.isTextblock) {
            const start = p + 1;
            const end = p + node.nodeSize - 1;
            if (best === null || Math.abs(start - pos) < Math.abs(best - pos)) best = start;
            if (Math.abs(end - pos) < Math.abs(best - pos)) best = end;
        }
        return true;
    });
    return new TextSelection(best === null ? 0 : best);
}

/** Marks that typing at the selection would produce. */
export function cursorMarks(state) {
    if (state.storedMarks) return state.storedMarks;
    const doc = state.doc;
    const sel = state.selection;
    if (!isTextSelection(sel)) return [];
    if (!sel.empty) return [];
    const $pos = doc.resolve(sel.from);
    const before = $pos.nodeBefore;
    if (before && (before.isText || before.isInline)) return before.marks;
    const after = $pos.nodeAfter;
    if (after && (after.isText || after.isInline)) return after.marks;
    return $pos.marks();
}

export class EditorState {
    constructor({ doc, selection, storedMarks = null, plugins = [], fields = {} }) {
        this.doc = doc;
        this.selection = selection || selectionNear(doc, 0);
        this.storedMarks = storedMarks;
        this.plugins = plugins;
        this.fields = fields;
    }

    static create({ doc, plugins = [] }) {
        const state = new EditorState({ doc, plugins });
        for (const plugin of plugins) {
            if (plugin.state) state.fields[plugin.key] = plugin.state.init(state);
        }
        state.selection = selectionNear(doc, 0);
        return state;
    }

    get schema() {
        return this.doc.type.schema;
    }

    tr() {
        return new Transaction(this);
    }

    apply(tr) {
        const fields = { ...this.fields };
        const next = new EditorState({
            doc: tr.doc,
            selection: tr.selectionSet
                ? tr.selection
                : mapSelection(this.selection, tr, this.doc),
            storedMarks: tr.storedMarksSet ? tr.storedMarks : clearStoredMarks(this, tr),
            plugins: this.plugins,
            fields,
        });
        for (const plugin of this.plugins) {
            if (plugin.state) {
                fields[plugin.key] = plugin.state.apply(tr, fields[plugin.key], this, next);
            }
        }
        return next;
    }
}

function mapSelection(selection, tr, _oldDoc) {
    const mapping = tr.mapping;
    if (selection.type === "node") {
        const r = mapping.map(selection.anchor, -1);
        return r.deleted ? selectionNear(tr.doc, r.pos) : new NodeSelection(r.pos);
    }
    const anchor = mapping.map(selection.anchor, -1).pos;
    const head = mapping.map(selection.head, 1).pos;
    return new TextSelection(anchor, head);
}

function clearStoredMarks(state, tr) {
    // Stored marks survive plain typing transactions but not
    // selection moves or structural changes.
    if (tr.selectionSet && !tr.docChanged) return null;
    return state.storedMarks;
}

/** A plugin: {key, state?: {init, apply}, props?: {...}}. */
export function createPlugin(spec) {
    return spec;
}

/** First plugin prop handler (in plugin order) that returns true wins. */
export function someProp(state, propName, ...args) {
    for (const plugin of state.plugins) {
        const fn = plugin.props && plugin.props[propName];
        if (fn && fn(...args)) return true;
    }
    return false;
}
