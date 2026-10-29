// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Our own editor view (substitute for prosemirror-view).
//
// The view renders the document as DOM under one contenteditable
// root and turns browser input back into transactions:
//
// - Rendering is per top-level block. A state change re-renders
//   only the blocks whose nodes actually changed (unchanged
//   subtrees keep their identity through the model), and kept
//   blocks after an edit get their stored positions shifted.
// - Typing arrives through `beforeinput` and is translated into
//   transactions before the browser touches the DOM, so the DOM
//   is always a projection of the state — never the source of it.
// - IME composition is the one exception: while composing, the
//   browser owns the DOM text, and compositionend commits the
//   result back as a text replacement. An `input` safety net
//   resyncs a block whose DOM text ever drifts from the state
//   (dictation, autocorrect, an input type we did not handle).
//
// Positions are tracked on the DOM nodes themselves (`_pos`): a
// text node knows the document position of its first character,
// an element knows the position just before its node.

import { Fragment, Slice } from "prosemirror-model";
import { NodeSelection, TextSelection, selectionNear } from "./state.js";
import { deleteSelection as deleteSelectionCmd, insertTextIn } from "./commands.js";
import { inlineSlice } from "./transform.js";
import { tokenize } from "./highlight.js";

// ---------------------------------------------------------------------------
// Rendering

function buildDOM(spec, fillContent) {
    if (typeof spec === "string") {
        const el = document.createElement(spec);
        fillContent(el);
        return el;
    }
    if (spec instanceof Node) return spec;
    const [tag, ...rest] = spec;
    const el = document.createElement(tag);
    for (const part of rest) {
        if (part === 0) {
            fillContent(el);
        } else if (Array.isArray(part)) {
            el.appendChild(buildDOM(part, fillContent));
        } else if (typeof part === "string") {
            el.appendChild(document.createTextNode(part));
        } else if (part && typeof part === "object") {
            for (const [name, value] of Object.entries(part)) {
                if (value == null) continue;
                if (name === "class") el.className = value;
                else if (name === "style") el.style.cssText = value;
                else el.setAttribute(name, value);
            }
        }
    }
    return el;
}

// The positioned atom ancestor of a DOM node, if any. Node views
// (hydrated attachments) contain foreign DOM whose nodes carry
// no _pos of their own; the atom owns their position.
function enclosingAtom(domNode) {
    let n = domNode.nodeType === 1 ? domNode : domNode.parentNode;
    while (n) {
        if (n.dataset && n.dataset.atom && n._pos !== undefined) return n;
        n = n.parentNode;
    }
    return null;
}

function assetUrl(id) {
    // §12: on Windows the scheme is served as an
    // http://<scheme>.localhost origin; everywhere else
    // the scheme form. The protocol handler reads the id
    // from the path either way.
    return navigator.userAgent.includes("Windows")
        ? `http://soirvu-asset.localhost/${id}`
        : `soirvu-asset://localhost/${id}`;
}

function markElement(mark) {
    const spec = mark.type.spec.toDOM(mark, true);
    return buildDOM(spec, () => { });
}

export class EditorView {
    constructor(el, options) {
        this.dom = el;
        this.state = options.state;
        this.dispatchTransaction = options.dispatchTransaction;
        this.editable = options.editable || (() => true);
        this.handleClickOn = options.handleClickOn || null;
        this.onTaskToggle = options.onTaskToggle || null;
        this.onPasteText = options.onPasteText || null;
        this.onAttachFile = options.onAttachFile || null;
        this.composing = false;
        this.compositionBlockPos = null;
        this.syncingSelection = false;
        this.lastHandledInputAt = 0;
        this.dragHandle = null;
        this.dragState = null;

        el.classList.add("ProseMirror");
        el.setAttribute("contenteditable", String(this.editable()));
        el.setAttribute("spellcheck", "true");
        el.setAttribute("role", "textbox");

        this.renderAll();
        this.bindEvents();
        this.syncSelection();
    }

    // -- rendering -----------------------------------------------------------

    renderAll() {
        this.dom.textContent = "";
        let pos = 0;
        for (let i = 0; i < this.state.doc.childCount; i++) {
            const child = this.state.doc.child(i);
            this.dom.appendChild(this.renderNode(child, pos));
            pos += child.nodeSize;
        }
    }

    /** A code block's text, split into highlighted spans.
     *  Every text node keeps its exact _pos, so position
     *  mapping and text sync see the same characters as the
     *  plain rendering — only the paint differs. */
    renderCodeText(child, block, parentEl, pos) {
        const spans = tokenize(child.text, block.attrs.params);
        if (!spans) {
            const domNode = document.createTextNode(child.text);
            domNode._pos = pos;
            parentEl.appendChild(domNode);
            return;
        }
        let cursor = 0;
        const emit = (text, offset, cls) => {
            if (!text) return;
            const domNode = document.createTextNode(text);
            domNode._pos = pos + offset;
            if (!cls) {
                parentEl.appendChild(domNode);
                return;
            }
            const span = document.createElement("span");
            span.className = "tok-" + cls;
            span.appendChild(domNode);
            parentEl.appendChild(span);
        };
        for (const span of spans) {
            emit(child.text.slice(cursor, span.start), cursor, null);
            emit(child.text.slice(span.start, span.end), span.start, span.cls);
            cursor = span.end;
        }
        emit(child.text.slice(cursor), cursor, null);
    }

    renderNode(node, pos) {
        const spec = node.type.spec.toDOM(node);
        let contentEl = null;
        const el = buildDOM(spec, (parent) => {
            contentEl = parent;
        });
        if (node.type.name === "image") {
            // §6.5: only attachment: images ever load, through the
            // asset protocol. A remote URL renders as a placeholder
            // and causes no network request.
            const src = node.attrs.src || "";
            if (src.startsWith("attachment:")) {
                el.setAttribute("src", assetUrl(src.slice("attachment:".length)));
                // An image this engine cannot decode (HEIC on
                // WebKitGTK or WebView2, say) fires error: mark
                // it, and the stylesheet shows a labelled
                // placeholder instead of the broken-image glyph.
                // The src stays, so double-click still opens
                // the file in the OS viewer (EntryEditor).
                el.addEventListener("error", () => {
                    el.setAttribute("data-unrenderable", "");
                    el.setAttribute(
                        "title",
                        `${el.getAttribute("alt") || "Image"} — cannot be shown here; double-click to open the file.`,
                    );
                });
            } else {
                el.removeAttribute("src");
                el.setAttribute("data-blocked-image", "");
            }
        }
        el._pos = pos;
        el._node = node;
        if (node.isAtom || node.isLeaf) {
            el.setAttribute("contenteditable", "false");
            el.dataset.atom = "true";
            return el;
        }
        const target = contentEl || el;
        this.renderContent(node, target, pos + 1);
        if (node.isTextblock && node.content.size === 0) {
            const br = document.createElement("br");
            br._pos = pos + 1;
            br._leaf = true;
            target.appendChild(br);
        }
        return el;
    }

    renderContent(node, parentEl, contentPos) {
        let pos = contentPos;
        node.content.forEach((child) => {
            if (child.isText && node.type.name === "code_block") {
                this.renderCodeText(child, node, parentEl, pos);
            } else if (child.isText) {
                let domNode = document.createTextNode(child.text);
                domNode._pos = pos;
                let wrapped = domNode;
                for (let i = child.marks.length - 1; i >= 0; i--) {
                    const wrapper = markElement(child.marks[i]);
                    wrapper.appendChild(wrapped);
                    wrapper._pos = pos;
                    wrapped = wrapper;
                }
                parentEl.appendChild(wrapped);
            } else if (child.isInline) {
                parentEl.appendChild(this.renderNode(child, pos));
            } else {
                parentEl.appendChild(this.renderNode(child, pos));
            }
            pos += child.nodeSize;
        });
    }

    reconcile(oldDoc, newDoc) {
        const oldKids = [];
        oldDoc.content.forEach((n) => oldKids.push(n));
        const newKids = [];
        newDoc.content.forEach((n) => newKids.push(n));
        let prefix = 0;
        while (
            prefix < oldKids.length &&
            prefix < newKids.length &&
            oldKids[prefix] === newKids[prefix]
        ) {
            prefix++;
        }
        let suffix = 0;
        while (
            suffix < oldKids.length - prefix &&
            suffix < newKids.length - prefix &&
            oldKids[oldKids.length - 1 - suffix] === newKids[newKids.length - 1 - suffix]
        ) {
            suffix++;
        }
        const oldMiddle = oldKids.length - prefix - suffix;
        const newMiddle = newKids.length - prefix - suffix;
        let oldMiddleSize = 0;
        for (let i = 0; i < oldMiddle; i++) oldMiddleSize += oldKids[prefix + i].nodeSize;
        let newMiddleSize = 0;
        for (let i = 0; i < newMiddle; i++) newMiddleSize += newKids[prefix + i].nodeSize;
        const delta = newMiddleSize - oldMiddleSize;

        // Remove the changed middle block elements.
        for (let i = 0; i < oldMiddle; i++) {
            const el = this.dom.childNodes[prefix];
            if (el) el.remove();
        }
        // Insert freshly rendered replacements.
        let pos = 0;
        for (let i = 0; i < prefix; i++) pos += newKids[i].nodeSize;
        const ref = this.dom.childNodes[prefix] || null;
        for (let i = 0; i < newMiddle; i++) {
            const child = newKids[prefix + i];
            const el = this.renderNode(child, pos);
            this.dom.insertBefore(el, ref);
            pos += child.nodeSize;
        }
        // Kept blocks after the edit keep their nodes but move.
        if (delta !== 0) {
            for (let i = prefix + newMiddle; i < newKids.length; i++) {
                const el = this.dom.childNodes[i];
                if (el) this.shiftPositions(el, delta);
            }
        }
    }

    shiftPositions(el, delta) {
        const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
        if (el._pos !== undefined) el._pos += delta;
        let node;
        while ((node = walker.nextNode())) {
            if (node._pos !== undefined) node._pos += delta;
        }
    }

    updateState(newState) {
        const oldDoc = this.state.doc;
        this.state = newState;
        if (newState.doc !== oldDoc && !this.composing) {
            this.reconcile(oldDoc, newState.doc);
        }
        if (!this.composing) this.syncSelection();
    }

    setEditable(value) {
        this.dom.setAttribute("contenteditable", String(value));
    }

    focus() {
        this.dom.focus();
        this.syncSelection();
    }

    destroy() {
        this.unbindEvents();
        if (this.dragHandle) this.dragHandle.remove();
        if (this.dropLine) this.dropLine.remove();
    }

    // -- block drag handles (Phase 7, §6.5) --------------------------------
    //
    // One handle element for the whole editor, fixed-positioned
    // in the gutter of the top-level block under the pointer,
    // and a drop line while it is dragged. Both live on
    // document.body, outside the editor tree: the reconciler
    // indexes the editor's children directly, so nothing
    // foreign may sit among them. Pointer events only — the
    // move is a single transaction, a single undo step.

    ensureDragEls() {
        if (this.dragHandle) return;
        const handle = document.createElement("div");
        handle.className = "block-drag-handle";
        handle.textContent = "\u283f";
        handle.contentEditable = "false";
        handle.style.display = "none";
        handle.addEventListener("mousedown", (e) => this.onDragStart(e));
        handle.addEventListener("mouseleave", () => this.hideDragHandle());
        document.body.appendChild(handle);
        const line = document.createElement("div");
        line.className = "block-drop-line";
        line.style.display = "none";
        document.body.appendChild(line);
        this.dragHandle = handle;
        this.dropLine = line;
    }

    onHoverBlock(e) {
        if (this.dragState || !this.editable()) return;
        const blockEl = this.topBlockOf(e.target);
        if (!blockEl || blockEl._pos === undefined) {
            this.hideDragHandle();
            return;
        }
        this.ensureDragEls();
        const rect = blockEl.getBoundingClientRect();
        const editorRect = this.dom.getBoundingClientRect();
        this.dragBlock = { el: blockEl, pos: blockEl._pos, node: blockEl._node };
        this.dragHandle.style.left = Math.max(0, editorRect.left - 22) + "px";
        this.dragHandle.style.top = rect.top + "px";
        this.dragHandle.style.display = "block";
    }

    hideDragHandle() {
        if (this.dragState) return;
        if (this.dragHandle) this.dragHandle.style.display = "none";
    }

    onDragStart(e) {
        if (!this.dragBlock || !this.editable()) return;
        e.preventDefault();
        e.stopPropagation();
        const blocks = [];
        let pos = 0;
        this.state.doc.content.forEach((node, _o, index) => {
            blocks.push({ index, pos, node });
            pos += node.nodeSize;
        });
        const from = blocks.findIndex((b) => b.pos === this.dragBlock.pos);
        if (from < 0) return;
        this.dragState = { blocks, from, target: from };
        this.dragMove = (ev) => this.onDragMove(ev);
        this.dragUp = () => this.onDragEnd();
        window.addEventListener("mousemove", this.dragMove);
        window.addEventListener("mouseup", this.dragUp);
    }

    dragBoundaryY(index) {
        const kids = this.dom.childNodes;
        if (index < kids.length) return kids[index].getBoundingClientRect().top - 2;
        return kids[kids.length - 1].getBoundingClientRect().bottom + 2;
    }

    onDragMove(e) {
        const ds = this.dragState;
        if (!ds) return;
        let target = ds.blocks.length;
        for (let i = 0; i < ds.blocks.length; i++) {
            const el = this.dom.childNodes[i];
            if (!el) continue;
            const rect = el.getBoundingClientRect();
            if (e.clientY < rect.top + rect.height / 2) {
                target = i;
                break;
            }
        }
        ds.target = target;
        const editorRect = this.dom.getBoundingClientRect();
        this.dropLine.style.left = editorRect.left + "px";
        this.dropLine.style.width = editorRect.width + "px";
        this.dropLine.style.top = this.dragBoundaryY(target) + "px";
        this.dropLine.style.display = "block";
    }

    onDragEnd() {
        const ds = this.dragState;
        this.dragState = null;
        window.removeEventListener("mousemove", this.dragMove);
        window.removeEventListener("mouseup", this.dragUp);
        if (this.dropLine) this.dropLine.style.display = "none";
        if (this.dragHandle) this.dragHandle.style.display = "none";
        if (!ds) return;
        const { blocks, from, target } = ds;
        if (target === from || target === from + 1) return;
        const node = blocks[from].node;
        const fromPos = blocks[from].pos;
        let insertAt = target < blocks.length ? blocks[target].pos : this.state.doc.content.size;
        if (insertAt > fromPos) insertAt -= node.nodeSize;
        const tr = this.state.tr();
        tr.delete(fromPos, fromPos + node.nodeSize);
        tr.insert(insertAt, [node]);
        tr.setSelection(selectionNear(tr.doc, insertAt + 1));
        this.dispatchTransaction(tr);
        this.focus();
    }

    // -- position mapping ------------------------------------------------------

    startPosOf(domNode) {
        if (domNode.nodeType === 3) {
            if (domNode._pos !== undefined) return domNode._pos;
            const atom = enclosingAtom(domNode);
            return atom ? atom._pos : undefined;
        }
        if (domNode._leaf) return domNode._pos;
        if (domNode.dataset && domNode.dataset.atom) return domNode._pos;
        if (domNode.firstChild) return this.startPosOf(domNode.firstChild);
        return domNode._pos !== undefined ? domNode._pos + 1 : 0;
    }

    endPosOf(domNode) {
        if (domNode.nodeType === 3) {
            if (domNode._pos !== undefined) return domNode._pos + domNode.textContent.length;
            const atom = enclosingAtom(domNode);
            return atom ? atom._pos + atom._node.nodeSize : undefined;
        }
        if (domNode._leaf) return domNode._pos;
        if (domNode.dataset && domNode.dataset.atom) return domNode._pos + domNode._node.nodeSize;
        if (domNode.lastChild) return this.endPosOf(domNode.lastChild);
        return domNode._pos !== undefined ? domNode._pos + 1 : 0;
    }

    posFromDOM(domNode, offset) {
        if (!this.dom.contains(domNode)) return this.state.selection.from;
        let result;
        if (domNode.nodeType === 3) {
            if (domNode._pos !== undefined) {
                result = domNode._pos + offset;
            } else {
                // Foreign text inside a node view: the atom owns
                // the position.
                const atom = enclosingAtom(domNode);
                result = atom ? atom._pos : this.state.selection.from;
            }
        } else {
            const kids = domNode.childNodes;
            if (kids.length === 0) {
                result = domNode._pos !== undefined ? domNode._pos + 1 : 0;
            } else if (offset >= kids.length) {
                result = this.endPosOf(kids[kids.length - 1]);
            } else {
                result = this.startPosOf(kids[offset]);
            }
        }
        // A position is a finite integer or it is nothing: a NaN
        // here would poison the state selection for every later
        // edit (inserts fail, deletes go nowhere).
        return Number.isFinite(result) ? result : this.state.selection.from;
    }

    candidates() {
        const out = [];
        const walker = document.createTreeWalker(
            this.dom,
            NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT,
        );
        let node;
        while ((node = walker.nextNode())) {
            if (node.nodeType === 3) {
                if (node._pos === undefined) continue;
                out.push({ node, from: node._pos, to: node._pos + node.textContent.length, leaf: false });
            } else if (node._leaf || (node.dataset && node.dataset.atom)) {
                const size = node._leaf ? 0 : node._node.nodeSize;
                out.push({ node, from: node._pos, to: node._pos + size, leaf: true });
            }
        }
        return out;
    }

    domPointForPos(pos) {
        const cands = this.candidates();
        for (let i = 0; i < cands.length; i++) {
            const c = cands[i];
            if (pos < c.from) {
                return { node: c.node.parentNode, offset: [...c.node.parentNode.childNodes].indexOf(c.node) };
            }
            if (pos <= c.to) {
                if (pos === c.to && i + 1 < cands.length && cands[i + 1].from === pos && !c.leaf) {
                    continue;
                }
                if (c.leaf) {
                    const parent = c.node.parentNode;
                    const idx = [...parent.childNodes].indexOf(c.node);
                    return { node: parent, offset: pos === c.from ? idx : idx + 1 };
                }
                return { node: c.node, offset: pos - c.from };
            }
        }
        const last = cands[cands.length - 1];
        if (last) return { node: last.node, offset: last.leaf ? 0 : last.to - last.from };
        return { node: this.dom, offset: 0 };
    }

    /** The client rect of a document position, so floating UI
     *  (the selection toolbar) can be placed from it (§6.5). A
     *  collapsed range on an empty line measures zero, so fall
     *  back to the containing element's box there. */
    coordsAtPos(pos) {
        const point = this.domPointForPos(pos);
        const range = document.createRange();
        range.setStart(point.node, point.offset);
        range.setEnd(point.node, point.offset);
        let rect = range.getBoundingClientRect();
        if (rect.height === 0 && rect.width === 0) {
            const el = point.node.nodeType === 3 ? point.node.parentNode : point.node;
            if (el && el.getBoundingClientRect) {
                const r = el.getBoundingClientRect();
                rect = { left: r.left, right: r.left, top: r.top, bottom: r.bottom };
            }
        }
        return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
    }

    syncSelection() {
        const domSel = document.getSelection();
        if (!domSel) return;
        if (domSel.rangeCount && !this.dom.contains(domSel.anchorNode) && document.activeElement !== this.dom) {
            return;
        }
        const sel = this.state.selection;
        this.syncingSelection = true;
        try {
            if (sel.type === "node") {
                const cands = this.candidates();
                const atom = cands.find((c) => c.leaf && c.from === sel.from && c.node._node);
                if (atom) {
                    const range = document.createRange();
                    range.selectNode(atom.node);
                    domSel.removeAllRanges();
                    domSel.addRange(range);
                }
                return;
            }
            const anchor = this.domPointForPos(sel.anchor);
            const head = this.domPointForPos(sel.head);
            const current = domSel.rangeCount
                ? {
                    anchorNode: domSel.anchorNode,
                    anchorOffset: domSel.anchorOffset,
                    focusNode: domSel.focusNode,
                    focusOffset: domSel.focusOffset,
                }
                : null;
            if (
                current &&
                current.anchorNode === anchor.node &&
                current.anchorOffset === anchor.offset &&
                current.focusNode === head.node &&
                current.focusOffset === head.offset
            ) {
                return;
            }
            if (typeof domSel.setBaseAndExtent === "function") {
                domSel.setBaseAndExtent(anchor.node, anchor.offset, head.node, head.offset);
            }
        } finally {
            this.syncingSelection = false;
        }
    }

    readDOMSelection() {
        const domSel = document.getSelection();
        if (!domSel || !domSel.rangeCount) return null;
        if (!this.dom.contains(domSel.anchorNode) || !this.dom.contains(domSel.focusNode)) return null;
        return {
            anchor: this.posFromDOM(domSel.anchorNode, domSel.anchorOffset),
            head: this.posFromDOM(domSel.focusNode, domSel.focusOffset),
        };
    }

    // -- events ----------------------------------------------------------------

    bindEvents() {
        this.handlers = {
            beforeinput: (e) => this.onBeforeInput(e),
            keydown: (e) => this.onKeyDown(e),
            input: (e) => this.onInputFallback(e),
            compositionstart: () => {
                this.composing = true;
            },
            compositionend: (e) => this.onCompositionEnd(e),
            paste: (e) => this.onPaste(e),
            mousedown: (e) => this.onMouseDown(e),
            mousemove: (e) => this.onHoverBlock(e),
            mouseleave: (e) => {
                // Moving onto the handle itself is not leaving:
                // it sits outside the editor, in the gutter.
                if (e.relatedTarget !== this.dragHandle) this.hideDragHandle();
            },
            click: (e) => this.onClick(e),
            drop: (e) => this.onDrop(e),
            dragover: (e) => e.preventDefault(),
        };
        for (const [name, fn] of Object.entries(this.handlers)) {
            this.dom.addEventListener(name, fn);
        }
        this.selectionHandler = () => this.onSelectionChange();
        document.addEventListener("selectionchange", this.selectionHandler);
    }

    unbindEvents() {
        if (!this.handlers) return;
        for (const [name, fn] of Object.entries(this.handlers)) {
            this.dom.removeEventListener(name, fn);
        }
        document.removeEventListener("selectionchange", this.selectionHandler);
        this.handlers = null;
    }

    dispatch(tr) {
        this.dispatchTransaction(tr);
    }

    pluginProp(propName, ...args) {
        for (const plugin of this.state.plugins) {
            const fn = plugin.props && plugin.props[propName];
            if (fn && fn(this.state, ...args, (tr) => this.dispatch(tr))) return true;
        }
        return false;
    }

    runKey(key, mods = {}) {
        const event = {
            key,
            ctrlKey: Boolean(mods.ctrl),
            metaKey: Boolean(mods.meta),
            altKey: Boolean(mods.alt),
            shiftKey: Boolean(mods.shift),
            preventDefault() { },
        };
        // Mod means Cmd on macOS, Ctrl elsewhere; the keymap
        // normalizes both to "Mod".
        const isMac = navigator.platform && navigator.platform.includes("Mac");
        if (mods.mod) {
            if (isMac) event.metaKey = true;
            else event.ctrlKey = true;
        }
        return this.pluginProp("handleKeyDown", event);
    }

    onKeyDown(e) {
        if (this.composing) return;
        if (this.pluginProp("handleKeyDown", e)) {
            e.preventDefault();
            this.lastHandledInputAt = performance.now();
        }
    }

    onBeforeInput(e) {
        if (this.composing) return;
        const state = this.state;
        const sel = state.selection;
        const handled = (fn) => {
            fn();
            e.preventDefault();
            this.lastHandledInputAt = performance.now();
        };
        switch (e.inputType) {
            case "insertText": {
                if (e.data == null) return;
                handled(() => {
                    const done = this.pluginProp("handleTextInput", sel.from, sel.to, e.data);
                    if (!done) {
                        const tr = state.tr();
                        insertTextIn(tr, state, e.data);
                        this.dispatch(tr);
                    }
                });
                return;
            }
            case "insertParagraph":
                handled(() => this.runKey("Enter"));
                return;
            case "insertLineBreak":
                handled(() => this.runKey("Enter", { shift: true }));
                return;
            case "deleteContentBackward":
                handled(() => this.runKey("Backspace"));
                return;
            case "deleteContentForward":
                handled(() => this.runKey("Delete"));
                return;
            case "deleteWordBackward":
                handled(() => this.runKey("Backspace", { alt: true }));
                return;
            case "deleteWordForward":
                handled(() => this.runKey("Delete", { alt: true }));
                return;
            case "deleteByCut":
                handled(() => deleteSelectionCmd(state, (tr) => this.dispatch(tr)));
                return;
            case "historyUndo":
                handled(() => this.runKey("z", { mod: true }));
                return;
            case "historyRedo":
                handled(() => this.runKey("Z", { mod: true, shift: true }));
                return;
            case "insertFromPaste":
            case "insertFromDrop": {
                const text = e.dataTransfer ? e.dataTransfer.getData("text/plain") : null;
                if (text != null && this.onPasteText) {
                    handled(() => this.onPasteText(text, state, (tr) => this.dispatch(tr)));
                }
                return;
            }
            case "insertReplacementText": {
                if (e.data == null) return;
                handled(() => {
                    const tr = state.tr();
                    insertTextIn(tr, state, e.data);
                    this.dispatch(tr);
                });
                return;
            }
            default:
                return;
        }
    }

    onDrop(e) {
        e.preventDefault();
        const files = e.dataTransfer ? e.dataTransfer.files : null;
        if (files && files.length && this.onAttachFile) {
            for (const file of files) this.onAttachFile(file);
        }
    }

    onPaste(e) {
        if (this.composing) return;
        const files = e.clipboardData ? e.clipboardData.files : null;
        if (files && files.length && this.onAttachFile) {
            e.preventDefault();
            for (const file of files) this.onAttachFile(file);
            return;
        }
        const text = e.clipboardData ? e.clipboardData.getData("text/plain") : null;
        if (text != null && this.onPasteText) {
            e.preventDefault();
            this.lastHandledInputAt = performance.now();
            this.onPasteText(text, this.state, (tr) => this.dispatch(tr));
        }
    }

    onCompositionEnd(e) {
        this.composing = false;
        // The browser edited one block's text. Commit it back.
        const anchor = document.getSelection()?.anchorNode;
        const blockEl = this.topBlockOf(anchor);
        if (blockEl && blockEl._node) {
            this.syncBlockText(blockEl);
        }
        this.updateState(this.state);
        void e;
    }

    topBlockOf(domNode) {
        let node = domNode;
        while (node && node.parentNode !== this.dom) node = node.parentNode;
        return node || null;
    }

    /** If the block element's DOM text differs from its state node,
     *  dispatch a transaction replacing the differing span. Only
     *  for textblocks whose content is pure text. */
    syncBlockText(blockEl) {
        const node = blockEl._node;
        const pos = blockEl._pos;
        if (!node || !node.isTextblock) return;
        let pure = true;
        node.content.forEach((child) => {
            if (!child.isText) pure = false;
        });
        if (!pure) return;
        const domText = blockEl.textContent;
        const stateText = node.textContent;
        if (domText === stateText) return;
        let prefix = 0;
        while (prefix < Math.min(domText.length, stateText.length) && domText[prefix] === stateText[prefix]) prefix++;
        let suffix = 0;
        while (
            suffix < Math.min(domText.length - prefix, stateText.length - prefix) &&
            domText[domText.length - 1 - suffix] === stateText[stateText.length - 1 - suffix]
        ) {
            suffix++;
        }
        const contentStart = pos + 1;
        const from = contentStart + prefix;
        const to = contentStart + stateText.length - suffix;
        const inserted = domText.slice(prefix, domText.length - suffix);
        // Marks for the inserted text: those of the state text at the cut.
        let marks = [];
        let offset = 0;
        node.content.forEach((child) => {
            if (prefix >= offset && prefix <= offset + child.nodeSize) marks = child.marks;
            offset += child.nodeSize;
        });
        const state = this.state;
        const tr = state.tr();
        const slice = inserted
            ? inlineSlice(Fragment.from(state.schema.text(inserted, marks)))
            : inlineSlice(Fragment.empty);
        tr.replaceRange(from, to, slice);
        tr.setSelection(new TextSelection(from + inserted.length));
        this.dispatch(tr);
    }

    onInputFallback() {
        if (this.composing) return;
        if (performance.now() - this.lastHandledInputAt < 50) return;
        const anchor = document.getSelection()?.anchorNode;
        const blockEl = this.topBlockOf(anchor);
        if (blockEl) this.syncBlockText(blockEl);
    }

    onClick(e) {
        // A task item's checkbox lives in the gutter left of its
        // text (EntryEditor's CSS draws it there). A click in the
        // gutter toggles the item; a click on the text just edits.
        const item = e.target.closest ? e.target.closest("li[data-checked]") : null;
        if (item && this.dom.contains(item) && item._pos !== undefined && this.onTaskToggle) {
            const text = item.querySelector("p") || item;
            if (e.clientX < text.getBoundingClientRect().left - 2) {
                e.preventDefault();
                this.onTaskToggle(item._pos);
                return;
            }
        }
        const link = e.target.closest ? e.target.closest("a[href]") : null;
        if (link && this.dom.contains(link)) {
            e.preventDefault();
            if (this.handleClickOn) this.handleClickOn(link.getAttribute("href"));
            return;
        }
    }

    // Pressing on an atom selects it in the model, synchronously:
    // mousedown always precedes the selection changes the press
    // causes, so no ordering with selectionchange or click has to
    // be guessed. The event is not prevented — media controls and
    // everything else still receive it normally.
    onMouseDown(e) {
        const atom = e.target.closest ? e.target.closest("[data-atom]") : null;
        if (!atom || !this.dom.contains(atom) || atom._pos === undefined) return;
        const pos = atom._pos;
        const sel = this.state.selection;
        if (sel.type === "node" && sel.anchor === pos) return;
        const tr = this.state.tr();
        tr.setSelection(new NodeSelection(pos));
        this.dispatch(tr);
    }

    onSelectionChange() {
        if (this.syncingSelection || this.composing) return;
        if (document.activeElement !== this.dom && !this.dom.contains(document.activeElement)) return;
        const read = this.readDOMSelection();
        if (!read) return;
        if (!Number.isFinite(read.anchor) || !Number.isFinite(read.head)) return;
        const sel = this.state.selection;
        if (sel.type !== "node" && read.anchor === read.head) {
            // A collapsed caret parked inside an atom (its content
            // is not editable text) means the atom is the
            // selection. Derived from the DOM node itself, so it
            // holds whatever order the events arrived in.
            const domSel = document.getSelection();
            const atomEl = domSel ? enclosingAtom(domSel.anchorNode) : null;
            if (atomEl && read.anchor === atomEl._pos) {
                const tr = this.state.tr();
                tr.setSelection(new NodeSelection(atomEl._pos));
                this.dispatch(tr);
                return;
            }
        }
        if (sel.type === "node") {
            // syncSelection writes a node selection into the DOM as
            // a range around the atom, and the browser may also park
            // the caret inside the atom's own DOM. Both are this
            // selection echoing back — replacing it with a text
            // selection here would make the atom undeletable, so
            // recognize the echo at the DOM level and keep it.
            const domSel = document.getSelection();
            const cands = this.candidates();
            const atom = cands.find((c) => c.leaf && c.from === sel.from && c.node._node);
            if (atom && domSel && domSel.rangeCount) {
                const { anchorNode: a, focusNode: f } = domSel;
                if ((a && atom.node.contains(a)) || (f && atom.node.contains(f))) return;
                const parent = atom.node.parentNode;
                if (parent) {
                    const idx = [...parent.childNodes].indexOf(atom.node);
                    const wraps =
                        (a === parent && f === parent &&
                            ((domSel.anchorOffset === idx && domSel.focusOffset === idx + 1) ||
                                (domSel.anchorOffset === idx + 1 && domSel.focusOffset === idx)));
                    if (wraps) return;
                }
            }
        }
        if (sel.type === "text" && sel.anchor === read.anchor && sel.head === read.head) return;
        // posFromDOM only yields text positions or atom boundaries,
        // so the raw positions are always representable.
        const tr = this.state.tr();
        tr.setSelection(new TextSelection(read.anchor, read.head));
        this.dispatch(tr);
    }
}
