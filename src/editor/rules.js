// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Input rules and the keymap (DESIGN §6.5) — our own code
// (substitutes for prosemirror-inputrules and prosemirror-keymap).
//
// Input rules turn typing into structure: "# " becomes a heading,
// "- " a list, "**word**" becomes bold as you close it. Nothing
// here reformats existing text — rules only fire on what is typed,
// which is what keeps the baseline rule (opening a note never
// rewrites it) intact.
//
// Deliberately not here: smart quotes and other typographic
// replacements. A diary keeps the characters its writer typed.
// No regex lookbehind anywhere: Safari before 16.4 cannot parse
// it, and the design supports macOS 12.

import { Fragment } from "prosemirror-model";
import { inlineSlice } from "./transform.js";
import { TextSelection, createPlugin, cursorMarks } from "./state.js";
import {
    chainCommands,
    createParagraphNear,
    deleteChar,
    deleteSelection,
    deleteWord,
    insertHardBreak,
    joinBackward,
    joinForward,
    liftEmptyBlock,
    liftListItem,
    newlineInCode,
    selectNodeBackward,
    selectNodeForward,
    setBlockType,
    sinkListItem,
    splitBlock,
    splitListItem,
    toggleMark,
    wrapBlocks,
} from "./commands.js";
import { redo, undo } from "./history.js";
import { enterInTable, tabInTable } from "./table.js";

export const rulesKey = "soirvu-rules";

// ---------------------------------------------------------------------------
// Rule machinery

/** Builds one transaction from a sequence of steps, where each
 *  step is a function (state) => transaction|null computed against
 *  the state produced by the previous steps. */
function buildCombined(state, builders) {
    const tr = state.tr();
    let current = state;
    try {
        for (const build of builders) {
            const part = build(current);
            if (!part) return null;
            for (const step of part.steps) tr.addStep(step);
            if (part.selectionSet) tr.setSelection(part.selection);
            if (part.storedMarksSet) tr.setStoredMarks(part.storedMarks);
            current = current.apply(part);
        }
        tr.doc.check();
    } catch {
        return null;
    }
    return tr;
}

function commandTr(state, command) {
    let captured = null;
    const ok = command(state, (tr) => (captured = tr));
    return ok ? captured : null;
}

/** A builder that deletes `count` characters of block text at the
 *  block's content start and puts the cursor there. */
function cutBlockStart(count) {
    return (state) => {
        const $from = state.doc.resolve(state.selection.from);
        const start = $from.start();
        const tr = state.tr();
        tr.delete(start, start + count);
        tr.setSelection(new TextSelection(start));
        return tr;
    };
}

function finishRule(state, tr) {
    if (!tr) return null;
    tr.setMeta(rulesKey, {
        inverses: tr.inverses,
        selectionBefore: state.selection,
        selectionAfter: tr.selection,
    });
    tr.setMeta("addToHistory", false);
    return tr;
}

// ---------------------------------------------------------------------------
// The rules themselves

function taskRule(state, $from, blockText, text) {
    if (text !== " ") return null;
    const m = blockText.match(/^\[([ x])\] $/);
    if (!m) return null;
    const doc = state.doc;
    const blockStart = $from.start();
    // The block must be the first block of a list item, and contain
    // nothing but the marker.
    if ($from.parent.textContent !== "[ ]" && $from.parent.textContent !== "[x]") return null;
    let itemDepth = -1;
    for (let d = $from.depth; d > 0; d--) {
        if ($from.node(d).type.name === "list_item") {
            itemDepth = d;
            break;
        }
    }
    if (itemDepth < 0) return null;
    if ($from.index(itemDepth) !== 0) return null; // not the item's first block
    const item = $from.node(itemDepth);
    const itemStart = $from.before(itemDepth);
    const tr = state.tr();
    const newFirst = $from.parent.type.create($from.parent.attrs, $from.parent.content.cut(3));
    const kids = [newFirst];
    for (let i = 1; i < item.childCount; i++) kids.push(item.child(i));
    const newItem = item.type.create(
        { ...item.attrs, checked: m[1] === "x" },
        Fragment.fromArray(kids),
    );
    tr.replaceWith(itemStart, itemStart + item.nodeSize, [newItem]);
    tr.setSelection(new TextSelection(blockStart));
    return finishRule(state, tr);
}

function blockRule(state, $from, blockText) {
    const schema = state.schema;
    const nodes = schema.nodes;
    const blockStart = $from.start();
    let m;

    if ((m = blockText.match(/^(#{1,3}) $/)) && nodes.heading) {
        const level = m[1].length;
        return finishRule(
            state,
            buildCombined(state, [
                cutBlockStart(level),
                (s) => commandTr(s, setBlockType(nodes.heading, { level })),
            ]),
        );
    }
    if (blockText === "> " && nodes.blockquote) {
        return finishRule(
            state,
            buildCombined(state, [
                cutBlockStart(1),
                (s) => commandTr(s, wrapBlocks([{ type: nodes.blockquote }])),
            ]),
        );
    }
    if (/^[-+*] $/.test(blockText) && nodes.bullet_list && nodes.list_item) {
        return finishRule(
            state,
            buildCombined(state, [
                cutBlockStart(1),
                (s) =>
                    commandTr(
                        s,
                        wrapBlocks([
                            { type: nodes.bullet_list, attrs: { tight: true } },
                            { type: nodes.list_item },
                        ]),
                    ),
            ]),
        );
    }
    if ((m = blockText.match(/^(\d+)\. $/)) && nodes.ordered_list && nodes.list_item) {
        const digits = m[1].length;
        return finishRule(
            state,
            buildCombined(state, [
                cutBlockStart(digits + 1),
                (s) =>
                    commandTr(
                        s,
                        wrapBlocks([
                            { type: nodes.ordered_list, attrs: { order: Number(m[1]), tight: true } },
                            { type: nodes.list_item },
                        ]),
                    ),
            ]),
        );
    }
    if (blockText === "```" && nodes.code_block) {
        return finishRule(
            state,
            buildCombined(state, [
                cutBlockStart(2),
                (s) => commandTr(s, setBlockType(nodes.code_block, { params: "" })),
            ]),
        );
    }
    if (
        blockText === "---" &&
        nodes.horizontal_rule &&
        nodes.paragraph &&
        $from.parent.type === nodes.paragraph &&
        $from.parent.textContent === "--"
    ) {
        const tr = state.tr();
        const before = blockStart - 1;
        const after = before + $from.parent.nodeSize;
        const hr = nodes.horizontal_rule.create();
        tr.replaceWith(before, after, [hr, nodes.paragraph.create()]);
        tr.setSelection(new TextSelection(before + hr.nodeSize + 1));
        return finishRule(state, tr);
    }
    return null;
}

const MARK_RULES = [
    { pattern: /\*\*([^*]+)\*\*$/, mark: "strong", markerLen: 2 },
    { pattern: /__([^_]+)__$/, mark: "strong", markerLen: 2 },
    { pattern: /~~([^~]+)~~$/, mark: "strike", markerLen: 2 },
    { pattern: /`([^`]+)`$/, mark: "code", markerLen: 1 },
    {
        pattern: /\*([^*]+)\*$/,
        mark: "em",
        markerLen: 1,
        // Not when the opener is the second half of a "**" — that
        // is the strong rule's, one keystroke later.
        guard: (text, m) => !(m.index > 0 && text[m.index - 1] === "*"),
    },
    {
        pattern: /_([^_]+)_$/,
        mark: "em",
        markerLen: 1,
        // No emphasis inside a word (snake_case stays literal).
        guard: (text, m) => !(m.index > 0 && /\w/.test(text[m.index - 1])),
    },
];

function markRule(state, $from, blockText) {
    const schema = state.schema;
    const blockStart = $from.start();
    for (const rule of MARK_RULES) {
        const markType = schema.marks[rule.mark];
        if (!markType) continue;
        const m = blockText.match(rule.pattern);
        if (!m) continue;
        if (rule.guard && !rule.guard(blockText, m)) continue;
        const inner = m[1];
        const matchStart = blockText.length - m[0].length;
        const from = blockStart + matchStart;
        const to = blockStart + blockText.length - 1; // the last char is being typed now
        // Never rewrite across an existing code span.
        let hasCode = false;
        state.doc.nodesBetween(from, to, (node) => {
            if (node.isText && schema.marks.code && schema.marks.code.isInSet(node.marks)) hasCode = true;
            return true;
        });
        if (hasCode) continue;
        const tr = state.tr();
        const textNode = schema.text(inner, [markType.create()]);
        tr.replaceRange(from, to, inlineSlice(Fragment.from(textNode)));
        tr.setSelection(new TextSelection(from + inner.length));
        tr.setStoredMarks(cursorMarks(state).filter((mk) => mk.type !== markType));
        return finishRule(state, tr);
    }
    return null;
}

/** The input-rules entry point: given the selection range the text
 *  is about to replace, decide whether the typed text triggers a
 *  rule. Returns a transaction or null. */
export function applyInputRules(state, from, to, text) {
    const sel = state.selection;
    if (sel.type !== "text" || !sel.empty) return null;
    if (sel.from !== from || sel.to !== to) return null;
    if (text.length !== 1) return null;
    const doc = state.doc;
    const $from = doc.resolve(from);
    if (!$from.parent.isTextblock) return null;
    if ($from.parent.type.spec.code) return null;
    const blockText = $from.parent.textContent.slice(0, $from.parentOffset) + text;
    return (
        taskRule(state, $from, blockText, text) ||
        blockRule(state, $from, blockText) ||
        markRule(state, $from, blockText)
    );
}

/** Backspace right after a rule fired undoes just the rule. */
export function undoInputRule(state, dispatch) {
    const field = state.fields[rulesKey];
    if (!field || !field.lastRule) return false;
    const { inverses, selectionBefore, selectionAfter } = field.lastRule;
    if (!state.selection.eq(selectionAfter)) return false;
    const tr = state.tr();
    try {
        for (let i = inverses.length - 1; i >= 0; i--) tr.addStep(inverses[i]);
        tr.doc.check();
    } catch {
        return false;
    }
    tr.setSelection(selectionBefore);
    tr.setMeta(rulesKey, "consumed");
    tr.setMeta("addToHistory", false);
    if (dispatch) dispatch(tr);
    return true;
}

export function inputRulesPlugin() {
    return createPlugin({
        key: rulesKey,
        state: {
            init() {
                return { lastRule: null };
            },
            apply(tr, field) {
                const meta = tr.getMeta(rulesKey);
                if (meta === "consumed") return { lastRule: null };
                if (meta) return { lastRule: meta };
                if (tr.docChanged || tr.selectionSet) return { lastRule: null };
                return field;
            },
        },
        props: {
            handleTextInput(state, from, to, text, dispatch) {
                const tr = applyInputRules(state, from, to, text);
                if (!tr) return false;
                dispatch(tr);
                return true;
            },
        },
    });
}

// ---------------------------------------------------------------------------
// Keymap

function keyNameCandidates(event) {
    const mods = [];
    if (event.ctrlKey || event.metaKey) mods.push("Mod");
    if (event.altKey) mods.push("Alt");
    if (event.shiftKey) mods.push("Shift");
    let base = event.key;
    const bases = new Set([base]);
    if (base.length === 1) {
        bases.add(base.toLowerCase());
        bases.add(base.toUpperCase());
    }
    if (base === " ") bases.add("Space");
    const prefixes = new Set([""]);
    const permute = (arr) =>
        arr.length <= 1
            ? [arr]
            : arr.flatMap((x, i) => permute([...arr.slice(0, i), ...arr.slice(i + 1)]).map((rest) => [x, ...rest]));
    for (const p of permute(mods)) prefixes.add(p.join("-") + "-");
    const out = [];
    for (const prefix of prefixes) for (const b of bases) out.push(prefix + b);
    // Most specific (most modifiers) first: Shift-Enter must win
    // over Enter, Shift-Mod-z over Mod-z's plain base, and so on.
    out.sort((a, b) => b.split("-").length - a.split("-").length);
    return out;
}

export function keymapPlugin(schema) {
    const marks = schema.marks;
    const table = {
        "Mod-z": undo,
        "Mod-y": redo,
        "Shift-Mod-z": redo,
        Enter: chainCommands(enterInTable, splitListItem, newlineInCode, liftEmptyBlock, splitBlock, createParagraphNear),
        "Shift-Enter": insertHardBreak,
        Backspace: chainCommands(undoInputRule, deleteSelection, deleteChar(-1), joinBackward, selectNodeBackward),
        Delete: chainCommands(deleteSelection, deleteChar(1), joinForward, selectNodeForward),
        "Mod-Backspace": deleteWord(-1),
        "Alt-Backspace": deleteWord(-1),
        "Mod-Delete": deleteWord(1),
        "Alt-Delete": deleteWord(1),
        Tab: chainCommands(tabInTable(1), sinkListItem),
        "Shift-Tab": chainCommands(tabInTable(-1), liftListItem),
    };
    if (marks.strong) table["Mod-b"] = toggleMark(marks.strong);
    if (marks.em) table["Mod-i"] = toggleMark(marks.em);
    if (marks.code) table["Mod-e"] = toggleMark(marks.code);
    if (marks.strike) table["Mod-Shift-x"] = toggleMark(marks.strike);
    return createPlugin({
        key: "soirvu-keymap",
        props: {
            handleKeyDown(state, event, dispatch) {
                for (const name of keyNameCandidates(event)) {
                    const command = table[name];
                    if (command && command(state, dispatch)) return true;
                }
                return false;
            },
        },
    });
}
