// Our own undo history (substitute for prosemirror-history).
//
// The plugin field keeps two stacks of events. An event is the
// steps of one transaction (or of several grouped transactions —
// typing comes in bursts, and undo should eat a burst, not a
// letter) plus, for each step, the inverse step the transaction
// recorded when it ran. Undo applies the inverses back to front
// and restores the selection from before the event; redo replays
// the steps and the selection after it. A fresh edit after undo
// clears the redo stack: history is a line, not a tree.

import { createPlugin, TextSelection } from "./state.js";

const GROUP_MS = 500;
const MAX_EVENTS = 500;

export const historyKey = "soirvu-history";

function isTypingEvent(tr) {
    if (tr.steps.length !== 1) return false;
    const step = tr.steps[0];
    // Pure insert (from === to) or small pure delete, no marks magic.
    return step.from === step.to || step.slice.size === 0;
}

function sameKind(a, b) {
    const aInsert = a.steps[0].from === a.steps[0].to;
    const bInsert = b.steps[0].from === b.steps[0].to;
    return aInsert === bInsert;
}

export function historyPlugin() {
    return createPlugin({
        key: historyKey,
        state: {
            init() {
                return { done: [], undone: [], lastTime: 0, groupOpen: false };
            },
            apply(tr, hist, oldState) {
                if (tr.getMeta("addToHistory") === false) return hist;
                const marker = tr.getMeta(historyKey);
                if (marker === "undo" || marker === "redo") {
                    return {
                        done: marker === "undo" ? hist.done.slice(0, -1) : [...hist.done, hist.undone[hist.undone.length - 1]],
                        undone: marker === "undo" ? [...hist.undone, hist.done[hist.done.length - 1]] : hist.undone.slice(0, -1),
                        lastTime: 0,
                        groupOpen: false,
                    };
                }
                if (!tr.docChanged) {
                    // A bare selection move closes the current group.
                    if (tr.selectionSet) return { ...hist, groupOpen: false };
                    return hist;
                }
                const event = {
                    steps: tr.steps,
                    inverses: tr.inverses,
                    selectionBefore: oldState.selection,
                    selectionAfter: tr.selection,
                };
                let done = hist.done;
                const canGroup =
                    hist.groupOpen &&
                    done.length > 0 &&
                    tr.time - hist.lastTime < GROUP_MS &&
                    isTypingEvent(tr) &&
                    sameKind(done[done.length - 1], event);
                if (canGroup) {
                    const last = done[done.length - 1];
                    const merged = {
                        steps: [...last.steps, ...event.steps],
                        inverses: [...last.inverses, ...event.inverses],
                        selectionBefore: last.selectionBefore,
                        selectionAfter: event.selectionAfter,
                    };
                    done = [...done.slice(0, -1), merged];
                } else {
                    done = [...done, event];
                    if (done.length > MAX_EVENTS) done = done.slice(done.length - MAX_EVENTS);
                }
                return { done, undone: [], lastTime: tr.time, groupOpen: true };
            },
        },
    });
}

function applySteps(state, dispatch, steps, selection, marker) {
    const tr = state.tr();
    try {
        for (const step of steps) tr.addStep(step);
        tr.doc.check();
    } catch {
        return false;
    }
    tr.setSelection(selection);
    tr.setMeta(historyKey, marker);
    if (dispatch) dispatch(tr);
    return true;
}

export function undo(state, dispatch) {
    const hist = state.fields[historyKey];
    if (!hist || hist.done.length === 0) return false;
    const event = hist.done[hist.done.length - 1];
    const steps = [];
    for (let i = event.inverses.length - 1; i >= 0; i--) steps.push(event.inverses[i]);
    return applySteps(state, dispatch, steps, event.selectionBefore, "undo");
}

export function redo(state, dispatch) {
    const hist = state.fields[historyKey];
    if (!hist || hist.undone.length === 0) return false;
    const event = hist.undone[hist.undone.length - 1];
    return applySteps(state, dispatch, event.steps, event.selectionAfter, "redo");
}

export { TextSelection };
