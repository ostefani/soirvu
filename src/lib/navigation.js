// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

/**
 * One entry-navigation history: pushes, replacements, back,
 * forward, reopening and disposal. The stack holds snapshots
 * ({entryId, scope, scoped, query, date, sel, scroll}); the
 * container captures snapshots from the UI and performs the
 * async settle/apply work, while every mutation and every
 * in-flight select goes through here so overlapping navigations
 * cannot corrupt the stack.
 *
 * A select that resolves after a newer select began is obsolete:
 * the container checks `isLiveSelect` after each await and drops
 * the stale one before pushing, applying or moving. History moves
 * land by reference (`arriveAt`), never by a pre-await offset, so
 * a push that slips in mid-flight cannot misplace the index.
 * Unknown ids need no coordinator check: selecting one loads the
 * entry pane, which reports deleted/missing itself.
 */
export function createNavigator({ onChange = null } = {}) {
    let stack = [];
    let index = -1;
    let generation = 0;
    let alive = true;

    function changed() {
        if (onChange) onChange();
    }

    return {
        get canGoBack() {
            return index > 0;
        },
        get canGoForward() {
            return index < stack.length - 1;
        },
        /** The current history entry, if any. */
        current() {
            return index >= 0 ? stack[index] : null;
        },
        /**
         * Records a navigation, discarding any forward tail. The
         * caller pushes only after its select survived settling.
         */
        push(entry) {
            stack = [...stack.slice(0, index + 1), entry];
            index = stack.length - 1;
            changed();
        },
        /**
         * Rewrites the current entry without touching the trail
         * (redirect-style navigations). Empty history pushes.
         */
        replace(entry) {
            if (index < 0) {
                stack = [entry];
                index = 0;
            } else {
                stack[index] = entry;
            }
            changed();
        },
        /**
         * Records leaving `from` for `entry`: finalizes the entry
         * left (an empty history adopts it, so the first navigation
         * away stays returnable) and makes the destination current,
         * discarding any forward tail. The caller passes both
         * snapshots only after its select survived settling.
         */
        pushNavigation(from, entry) {
            this.replace(from);
            this.push(entry);
        },
        /**
         * The back/forward target without moving. Null when the
         * step leaves the stack — the caller does nothing then.
         */
        peek(delta) {
            const next = index + delta;
            if (next < 0 || next >= stack.length) return null;
            return stack[next];
        },
        /**
         * Lands on a previously peeked entry after its select
         * succeeded. By reference, so a push that slipped in
         * mid-flight moves to the entry actually shown instead of
         * a stale offset. Returns whether it landed.
         */
        arriveAt(entry) {
            const at = stack.indexOf(entry);
            if (at < 0) return false;
            index = at;
            changed();
            return true;
        },
        /** Forgets the whole trail (vault switch, lock). */
        reset() {
            generation++;
            stack = [];
            index = -1;
            changed();
        },
        /**
         * Starts a select; later selects retire earlier ones.
         * @returns {number} the token to check on every settle.
         */
        beginSelect() {
            return ++generation;
        },
        /** True while no newer select began and the owner lives. */
        isLiveSelect(token) {
            return alive && token === generation;
        },
        /** Permanently retires the owner: torn-down panes navigate nothing. */
        dispose() {
            alive = false;
            generation++;
        },
    };
}
