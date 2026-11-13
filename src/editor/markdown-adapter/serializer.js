// SPDX-License-Identifier: MIT
// Adapted from prosemirror-markdown 1.13.8. See LICENSE in this directory.
import { Mark } from "prosemirror-model";

export const commonNodes = {
    blockquote(state, node) {
        state.wrapBlock("> ", null, node, () => state.renderContent(node));
    },
    code_block(state, node) {
        // Use a fence longer than any run of backticks in the code.
        const backticks = node.textContent.match(/`{3,}/gm);
        const fence = backticks ? (backticks.sort().slice(-1)[0] + "`") : "```";
        state.write(fence + (node.attrs.params || "") + "\n");
        state.text(node.textContent, false);
        // Add a newline to the current content before adding closing marker
        state.write("\n");
        state.write(fence);
        state.closeBlock(node);
    },
    horizontal_rule(state, node) {
        state.write(node.attrs.markup || "---");
        state.closeBlock(node);
    },
    bullet_list(state, node) {
        state.renderList(node, "  ", () => (node.attrs.bullet || "*") + " ");
    },
    ordered_list(state, node) {
        const start = node.attrs.order ?? 1;
        let maxW = String(start + node.childCount - 1).length;
        let space = " ".repeat(maxW + 2);
        state.renderList(node, space, i => {
            let nStr = String(start + i);
            return " ".repeat(maxW - nStr.length) + nStr + ". ";
        });
    },
    image(state, node) {
        state.write("![" + state.esc(node.attrs.alt || "") + "](" + node.attrs.src.replace(/[\(\)]/g, "\\$&") +
            (node.attrs.title ? ' "' + node.attrs.title.replace(/"/g, '\\"') + '"' : "") + ")");
    },
};

export const commonMarks = {
    em: { open: "*", close: "*", mixable: true, expelEnclosingWhitespace: true },
    strong: { open: "**", close: "**", mixable: true, expelEnclosingWhitespace: true },
    link: {
        open(state, mark, parent, index) {
            state.inAutolink = isPlainURL(mark, parent, index);
            return state.inAutolink ? "<" : "[";
        },
        close(state, mark, parent, index) {
            let { inAutolink } = state;
            state.inAutolink = undefined;
            return inAutolink ? ">"
                : "](" + mark.attrs.href.replace(/[\(\)"]/g, "\\$&") + (mark.attrs.title ? ` "${mark.attrs.title.replace(/"/g, '\\"')}"` : "") + ")";
        },
        mixable: true
    },
    code: { open(_state, _mark, parent, index) { return backticksFor(parent.child(index), -1); },
        close(_state, _mark, parent, index) { return backticksFor(parent.child(index - 1), 1); },
        escape: false }
};

function backticksFor(node, side) {
    let ticks = /`+/g, m, len = 0;
    if (node.isText)
        while (m = ticks.exec(node.text))
            len = Math.max(len, m[0].length);
    let result = len > 0 && side > 0 ? " `" : "`";
    for (let i = 0; i < len; i++)
        result += "`";
    if (len > 0 && side < 0)
        result += " ";
    return result;
}
function isPlainURL(link, parent, index) {
    if (link.attrs.title || !/^\w+:/.test(link.attrs.href))
        return false;
    let content = parent.child(index);
    if (!content.isText || content.text != link.attrs.href || content.marks[content.marks.length - 1] != link)
        return false;
    return index == parent.childCount - 1 || !link.isInSet(parent.child(index + 1).marks);
}
/**
This is an object used to track state and expose
methods related to markdown serialization. Instances are passed to
node and mark serialization methods (see `toMarkdown`).
*/
class MarkdownSerializerState {
    constructor(nodes, marks, tightLists) {
        this.nodes = nodes;
        this.marks = marks;
        this.tightLists = tightLists;
        this.delim = "";
        this.out = "";
        this.closed = null;
        this.inAutolink = undefined;
        this.atBlockStart = false;
        this.inTightList = false;
    }
    flushClose(size = 2) {
        if (this.closed) {
            if (!this.atBlank())
                this.out += "\n";
            if (size > 1) {
                let delimMin = this.delim;
                let trim = /\s+$/.exec(delimMin);
                if (trim)
                    delimMin = delimMin.slice(0, delimMin.length - trim[0].length);
                for (let i = 1; i < size; i++)
                    this.out += delimMin + "\n";
            }
            this.closed = null;
        }
    }
    getMark(name) {
        const info = this.marks[name];
        if (!info) throw new Error(`Mark type \`${name}\` not supported by Markdown renderer`);
        return info;
    }
    /**
    Render a block, prefixing each line with `delim`, and the first
    line in `firstDelim`. `node` should be the node that is closed at
    the end of the block, and `f` is a function that renders the
    content of the block.
    */
    wrapBlock(delim, firstDelim, node, f) {
        let old = this.delim;
        this.write(firstDelim != null ? firstDelim : delim);
        this.delim += delim;
        f();
        this.delim = old;
        this.closeBlock(node);
    }
    atBlank() {
        return /(^|\n)$/.test(this.out);
    }
    /**
    Prepare the state for writing output (closing closed paragraphs,
    adding delimiters, and so on), and then optionally add content
    (unescaped) to the output.
    */
    write(content) {
        this.flushClose();
        if (this.delim && this.atBlank())
            this.out += this.delim;
        if (content)
            this.out += content;
    }
    /**
    Close the block for the given node.
    */
    closeBlock(node) {
        this.closed = node;
    }
    /**
    Add the given text to the document. When escape is not `false`,
    it will be escaped.
    */
    text(text, escape = true) {
        let lines = text.split("\n");
        for (let i = 0; i < lines.length; i++) {
            this.write();
            // Escape exclamation marks in front of links
            if (!escape && lines[i][0] == "[" && /(^|[^\\])\!$/.test(this.out))
                this.out = this.out.slice(0, this.out.length - 1) + "\\!";
            this.out += escape ? this.esc(lines[i], this.atBlockStart) : lines[i];
            if (i != lines.length - 1)
                this.out += "\n";
        }
    }
    /**
    Render the given node as a block.
    */
    render(node, parent, index) {
        const render = this.nodes[node.type.name];
        if (!render) throw new Error(`Token type ${node.type.name} not supported by Markdown renderer`);
        render(this, node, parent, index);
    }
    /**
    Render the contents of `parent` as block nodes.
    */
    renderContent(parent) {
        parent.forEach((node, _, i) => this.render(node, parent, i));
    }
    /**
    Render the contents of `parent` as inline content.
    */
    renderInline(parent, fromBlockStart = true) {
        this.atBlockStart = fromBlockStart;
        let active = [], trailing = "";
        let progress = (node, offset, index) => {
            let marks = node ? node.marks : [];
            // Remove marks from `hard_break` that are the last node inside
            // that mark to prevent parser edge cases with new lines just
            // before closing marks.
            if (node && node.type.name === "hard_break")
                marks = marks.filter(m => {
                    if (index + 1 == parent.childCount)
                        return false;
                    let next = parent.child(index + 1);
                    return m.isInSet(next.marks) && (!next.isText || /\S/.test(next.text));
                });
            let inner = marks.length ? marks[marks.length - 1] : null;
            let noEsc = inner && this.getMark(inner.type.name).escape === false;
            let len = marks.length - (noEsc ? 1 : 0);
            // Try to reorder 'mixable' marks, such as em and strong, which
            // in Markdown may be opened and closed in different order, so
            // that order of the marks for the token matches the order in
            // active.
            outer: for (let i = 0; i < len; i++) {
                let mark = marks[i];
                if (!this.getMark(mark.type.name).mixable)
                    break;
                for (let j = 0; j < active.length; j++) {
                    let other = active[j];
                    if (!this.getMark(other.type.name).mixable)
                        break;
                    if (mark.eq(other)) {
                        if (i > j)
                            marks = marks.slice(0, j).concat(mark).concat(marks.slice(j, i)).concat(marks.slice(i + 1, len));
                        else if (j > i)
                            marks = marks.slice(0, i).concat(marks.slice(i + 1, j)).concat(mark).concat(marks.slice(j, len));
                        continue outer;
                    }
                }
            }
            // Find the prefix of the mark set that didn't change
            let keep = 0;
            while (keep < Math.min(active.length, len) && marks[keep].eq(active[keep]))
                ++keep;
            let leading = trailing;
            trailing = "";
            // If whitespace has to be expelled from the node, adjust
            // leading and trailing accordingly.
            if (node && node.isText && marks.some(mark => {
                let info = this.getMark(mark.type.name);
                return info && info.expelEnclosingWhitespace && !active.some((m, i) => i < keep && m.eq(mark));
            })) {
                let [_, lead, rest] = /^(\s*)(.*)$/m.exec(node.text);
                if (lead) {
                    leading += lead;
                    node = rest ? node.withText(rest) : null;
                    if (!node)
                        marks = active;
                }
            }
            if (node && node.isText && marks.some((mark, i) => {
                let info = this.getMark(mark.type.name);
                return info && info.expelEnclosingWhitespace && !this.isMarkAhead(parent, index + 1, marks.slice(0, i + 1));
            })) {
                let [_, rest, trail] = /^(.*?)(\s*)$/m.exec(node.text);
                if (trail) {
                    trailing = trail;
                    node = rest ? node.withText(rest) : null;
                    if (!node)
                        marks = active;
                }
            }
            // Close the marks that need to be closed
            if (node || index == parent.childCount)
                while (keep < active.length)
                    this.text(this.markString(active.pop(), false, parent, index), false);
            // Output any previously expelled trailing whitespace outside the marks
            if (leading)
                this.text(leading);
            // Open the marks that need to be opened
            if (node) {
                while (active.length < len) {
                    let add = marks[active.length];
                    active.push(add);
                    this.text(this.markString(add, true, parent, index), false);
                    this.atBlockStart = false;
                }
                // Render the node. Special case code marks, since their content
                // may not be escaped.
                if (noEsc && node.isText)
                    this.text(this.markString(inner, true, parent, index) + node.text +
                        this.markString(inner, false, parent, index + 1), false);
                else
                    this.render(node, parent, index);
                this.atBlockStart = false;
                // After the first non-empty text node is rendered, the end of output
                // is no longer at block start.
                //
                // FIXME: If a non-text node writes something to the output for this
                // block, the end of output is also no longer at block start. But how
                // can we detect that?
                if (node.isText && node.nodeSize > 0)
                    this.atBlockStart = false;
            }
        };
        parent.forEach(progress);
        progress(null, 0, parent.childCount);
        this.atBlockStart = false;
    }
    /**
    Render a node's content as a list. `delim` should be the extra
    indentation added to all lines except the first in an item,
    `firstDelim` is a function going from an item index to a
    delimiter for the first line of the item.
    */
    renderList(node, delim, firstDelim) {
        if (this.closed && this.closed.type == node.type)
            this.flushClose(3);
        else if (this.inTightList)
            this.flushClose(1);
        let isTight = typeof node.attrs.tight != "undefined" ? node.attrs.tight : this.tightLists;
        let prevTight = this.inTightList;
        this.inTightList = isTight;
        node.forEach((child, _, i) => {
            if (i && isTight)
                this.flushClose(1);
            this.wrapBlock(delim, firstDelim(i), node, () => this.render(child, node, i));
        });
        this.inTightList = prevTight;
    }
    /**
    Escape the given string so that it can safely appear in Markdown
    content. If `startOfLine` is true, also escape characters that
    have special meaning only at the start of the line.
    */
    esc(str, startOfLine = false) {
        str = str.replace(/[`*\\~\[\]_]/g, (m, i) => m == "_" && i > 0 && i + 1 < str.length && str[i - 1].match(/\w/) && str[i + 1].match(/\w/) ? m : "\\" + m);
        if (startOfLine)
            str = str.replace(/^(\+[ ]|[\-*>])/, "\\$&").replace(/^(\s*)(#{1,6})(\s|$)/, '$1\\$2$3').replace(/^(\s*\d+)\.\s/, "$1\\. ");
        return str;
    }
    /**
    Get the markdown string for a given opening or closing mark.
    */
    markString(mark, open, parent, index) {
        let info = this.getMark(mark.type.name);
        let value = open ? info.open : info.close;
        return typeof value == "string" ? value : value(this, mark, parent, index);
    }
    isMarkAhead(parent, index, marks) {
        for (;; index++) {
            if (index >= parent.childCount)
                return false;
            let next = parent.child(index);
            if (next.type.name != "hard_break") {
                return next.marks.length >= marks.length && Mark.sameSet(next.marks.slice(0, marks.length), marks);
            }
            index++;
        }
    }
}


export class MarkdownSerializer {
    constructor(nodes, marks) {
        this.nodes = nodes;
        this.marks = marks;
    }

    serialize(content, { tightLists = false } = {}) {
        const state = new MarkdownSerializerState(this.nodes, this.marks, tightLists);
        state.renderContent(content);
        return state.out;
    }
}
