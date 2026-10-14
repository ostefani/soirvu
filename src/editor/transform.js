// Our own small transform layer (substitute for prosemirror-transform).
//
// One step type does almost everything we need: replace a range of
// the document with a slice. Block-type changes, wraps, lifts, joins
// and splits are all expressed by the commands as replacements of a
// block range with rebuilt nodes, computed from the document itself.
// Each transaction also records, for every step, the slice it removed
// — that is what makes steps invertible for undo (history.js)
// without storing whole documents.

import { Fragment, Slice } from "prosemirror-model";

export class ReplaceStep {
    constructor(from, to, slice) {
        this.from = from;
        this.to = to;
        this.slice = slice;
    }

    apply(doc) {
        try {
            return { doc: doc.replace(this.from, this.to, this.slice) };
        } catch (e) {
            return { failed: String(e && e.message ? e.message : e) };
        }
    }

    /** The step that undoes this one, given the document it ran on. */
    invert(docBefore) {
        return new ReplaceStep(
            this.from,
            this.from + this.slice.size,
            docBefore.slice(this.from, this.to),
        );
    }

    map(pos, assoc = 1) {
        if (pos < this.from) return { pos, deleted: false };
        if (pos > this.to) {
            return { pos: pos + (this.slice.size - (this.to - this.from)), deleted: false };
        }
        const side = assoc <= 0 ? this.from : this.from + this.slice.size;
        return { pos: side, deleted: pos > this.from && pos < this.to };
    }
}

export class Mapping {
    constructor(maps = []) {
        this.maps = maps;
    }

    map(pos, assoc = 1) {
        let deleted = false;
        for (const step of this.maps) {
            const r = step.map(pos, assoc);
            pos = r.pos;
            if (r.deleted) deleted = true;
        }
        return { pos, deleted };
    }
}

/** A slice holding exactly the given nodes, closed on both sides. */
export function closedSlice(nodes) {
    return new Slice(Fragment.fromArray(nodes), 0, 0);
}

/** A slice holding inline content (for replacing inside a textblock). */
export function inlineSlice(fragment) {
    return new Slice(fragment, 0, 0);
}

export class Transaction {
    constructor(state) {
        this.doc = state.doc;
        this.selection = state.selection;
        this.storedMarks = state.storedMarks;
        this.steps = [];
        this.inverses = [];
        this.meta = {};
        this.selectionSet = false;
        this.storedMarksSet = false;
        this.time = Date.now();
    }

    /** Applies the step immediately; throws if the step fails. */
    addStep(step) {
        const result = step.apply(this.doc);
        if (result.failed) throw new Error(result.failed);
        this.inverses.push(step.invert(this.doc));
        this.doc = result.doc;
        this.steps.push(step);
    }

    replaceWith(from, to, nodes) {
        this.addStep(new ReplaceStep(from, to, closedSlice(nodes)));
        return this;
    }

    replaceRange(from, to, slice) {
        this.addStep(new ReplaceStep(from, to, slice));
        return this;
    }

    insert(pos, nodes) {
        return this.replaceWith(pos, pos, nodes);
    }

    delete(from, to) {
        return this.replaceWith(from, to, []);
    }

    setSelection(selection) {
        this.selection = selection;
        this.selectionSet = true;
        return this;
    }

    setStoredMarks(marks) {
        this.storedMarks = marks;
        this.storedMarksSet = true;
        return this;
    }

    setMeta(key, value) {
        this.meta[key] = value;
        return this;
    }

    getMeta(key) {
        return this.meta[key];
    }

    get mapping() {
        return new Mapping(this.steps);
    }

    get docChanged() {
        return this.steps.length > 0;
    }
}
