// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

/**
 * Latest-request ownership for one read purpose: search, folder
 * loading, tag counts, a calendar month. Each `begin` retires
 * every earlier token, so a response that arrives after a newer
 * request started — or after the owner was disposed — is
 * discarded instead of published. One owner per purpose: a new
 * search must never cancel an unrelated folder refresh.
 *
 * A generation check discards obsolete responses; it does not
 * cancel a backend mutation already submitted. Committed mutation
 * results still reconcile through their own paths.
 */
export function createRequestOwner() {
    let generation = 0;
    let alive = true;

    return {
        /**
         * Starts a request, retiring every earlier token.
         * @returns {number} the token to check when this request settles.
         */
        begin() {
            return ++generation;
        },
        /**
         * True while no newer request began and the owner lives.
         * Check before publishing rows, errors and pending-state
         * cleanup alike — a stale failure must not clear current data.
         */
        current(token) {
            return alive && token === generation;
        },
        /** Retires outstanding requests: new intent, reset, vault change. */
        invalidate() {
            generation++;
        },
        /** Permanently retires the owner: disposed owners publish nothing. */
        dispose() {
            alive = false;
            generation++;
        },
    };
}
