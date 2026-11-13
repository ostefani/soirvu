// SPDX-License-Identifier: MIT
// Adapted from prosemirror-markdown 1.13.8. See LICENSE in this directory.
import { Mark } from "prosemirror-model";

function maybeMerge(a, b) {
    if (a.isText && b.isText && Mark.sameSet(a.marks, b.marks))
        return a.withText(a.text + b.text);
}
// Object used to track the context of a running parse.
class MarkdownParseState {
    constructor(schema, tokenHandlers) {
        this.schema = schema;
        this.tokenHandlers = tokenHandlers;
        this.stack = [{ type: schema.topNodeType, attrs: null, content: [], marks: Mark.none }];
    }
    top() {
        return this.stack[this.stack.length - 1];
    }
    push(elt) {
        if (this.stack.length)
            this.top().content.push(elt);
    }
    // Adds the given text to the current position in the document,
    // using the current marks as styling.
    addText(text) {
        if (!text)
            return;
        let top = this.top(), nodes = top.content, last = nodes[nodes.length - 1];
        let node = this.schema.text(text, top.marks), merged;
        if (last && (merged = maybeMerge(last, node)))
            nodes[nodes.length - 1] = merged;
        else
            nodes.push(node);
    }
    // Adds the given mark to the set of active marks.
    openMark(mark) {
        let top = this.top();
        top.marks = mark.addToSet(top.marks);
    }
    // Removes the given mark from the set of active marks.
    closeMark(mark) {
        let top = this.top();
        top.marks = mark.removeFromSet(top.marks);
    }
    parseTokens(toks) {
        for (let i = 0; i < toks.length; i++) {
            let tok = toks[i];
            let handler = this.tokenHandlers[tok.type];
            if (!handler)
                throw new Error("Token type `" + tok.type + "` not supported by Markdown parser");
            handler(this, tok, toks, i);
        }
    }
    // Add a node at the current position.
    addNode(type, attrs, content) {
        let top = this.top();
        let node = type.createAndFill(attrs, content, top ? top.marks : []);
        if (!node)
            return null;
        this.push(node);
        return node;
    }
    // Wrap subsequent content in a node of the given type.
    openNode(type, attrs) {
        this.stack.push({ type: type, attrs: attrs, content: [], marks: Mark.none });
    }
    // Close and return the node that is currently on top of the stack.
    closeNode() {
        let info = this.stack.pop();
        return this.addNode(info.type, info.attrs, info.content);
    }
}
function attrs(spec, token, tokens, i) {
    return spec.getAttrs?.(token, tokens, i);
}
function withoutTrailingNewline(str) {
    return str[str.length - 1] == "\n" ? str.slice(0, str.length - 1) : str;
}
function tokenHandlers(schema, tokens) {
    let handlers = Object.create(null);
    for (let type in tokens) {
        let spec = tokens[type];
        if (spec.block) {
            let nodeType = schema.nodeType(spec.block);
            if (spec.noCloseToken) {
                handlers[type] = (state, tok, tokens, i) => {
                    state.openNode(nodeType, attrs(spec, tok, tokens, i));
                    state.addText(withoutTrailingNewline(tok.content));
                    state.closeNode();
                };
            }
            else {
                handlers[type + "_open"] = (state, tok, tokens, i) => state.openNode(nodeType, attrs(spec, tok, tokens, i));
                handlers[type + "_close"] = state => state.closeNode();
            }
        }
        else if (spec.node) {
            let nodeType = schema.nodeType(spec.node);
            handlers[type] = (state, tok, tokens, i) => state.addNode(nodeType, attrs(spec, tok, tokens, i));
        }
        else if (spec.mark) {
            let markType = schema.marks[spec.mark];
            if (spec.noCloseToken) {
                handlers[type] = (state, tok, tokens, i) => {
                    state.openMark(markType.create(attrs(spec, tok, tokens, i)));
                    state.addText(withoutTrailingNewline(tok.content));
                    state.closeMark(markType);
                };
            }
            else {
                handlers[type + "_open"] = (state, tok, tokens, i) => state.openMark(markType.create(attrs(spec, tok, tokens, i)));
                handlers[type + "_close"] = state => state.closeMark(markType);
            }
        }
        else {
            throw new RangeError("Unrecognized parsing spec " + JSON.stringify(spec));
        }
    }
    handlers.text = (state, tok) => state.addText(tok.content);
    handlers.inline = (state, tok) => state.parseTokens(tok.children);
    handlers.softbreak = state => state.addText(" ");
    return handlers;
}
/**
A configuration of a Markdown parser. Such a parser uses
[markdown-it](https://github.com/markdown-it/markdown-it) to
tokenize a file, and then runs the custom rules it is given over
the tokens to create a ProseMirror document tree.
*/
export class MarkdownParser {
    constructor(schema, tokenizer, tokens) {
        this.schema = schema;
        this.tokenizer = tokenizer;
        this.tokenHandlers = tokenHandlers(schema, tokens);
    }

    parse(text, markdownEnv = {}) {
        let state = new MarkdownParseState(this.schema, this.tokenHandlers), doc;
        state.parseTokens(this.tokenizer.parse(text, markdownEnv));
        do {
            doc = state.closeNode();
        } while (state.stack.length);
        return doc || this.schema.topNodeType.createAndFill();
    }
}
