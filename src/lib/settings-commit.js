// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

/**
 * Serialized application-preference writes. Each patch merges
 * against the latest acknowledged snapshot when its turn starts —
 * never against the snapshot that was current when the user
 * clicked. Two rapid toggles therefore persist both changes
 * instead of the second reverting the first. File reading, native
 * dialogs and reads stay outside; only the revision-sensitive
 * tail (merge, write, re-read) goes through here.
 *
 * This is deliberately not the entry commit queue: unrelated
 * domains must not share one sequence, and settings revisions
 * are whole-object replacements with no expected-revision check.
 * A task's rejection leaves the acknowledged snapshot untouched,
 * so the failed patch stays retryable against last-good state.
 *
 * Every mutation must arrive as a patch — including folder
 * picker/clear results, which the caller re-saves as a one-field
 * patch instead of adopting the carried snapshot. A snapshot
 * taken outside the sequence has no ordering against queued
 * writes, so adopting one mid-session can silently revert a
 * preference a save just stored; the mount load is the only
 * adoption, passed as the initial snapshot.
 */
export function createSettingsCommit(acknowledged, { set, get }) {
    let current = acknowledged;
    // Set when a write reached the backend but the confirming
    // re-read failed: the base is uncertain, so the next write
    // must reconcile with a fresh read first instead of merging
    // over a snapshot the backend has already moved past.
    let unreconciled = false;
    let tail = Promise.resolve();

    return {
        /**
         * Merges `patch` over the latest acknowledged snapshot,
         * writes the merged object, then re-reads the canonical
         * snapshot. A write whose re-read failed leaves the base
         * uncertain; the next save reconciles first and refuses
         * while reconciliation itself fails — never a blind write
         * over unknown state. Resolves with the acknowledged
         * settings.
         */
        save(patch) {
            const run = tail.then(async () => {
                if (unreconciled) {
                    current = await get();
                    unreconciled = false;
                }
                const merged = { ...current, ...patch };
                await set(merged);
                try {
                    current = await get();
                } catch (e) {
                    unreconciled = true;
                    throw e;
                }
                return current;
            });
            tail = run.catch(() => {});
            return run;
        },
    };
}
