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
 */
export function createSettingsCommit(acknowledged, { set, get }) {
    let current = acknowledged;
    let tail = Promise.resolve();

    return {
        /**
         * Merges `patch` over the latest acknowledged snapshot,
         * writes the merged object, then re-reads the canonical
         * snapshot. Resolves with the acknowledged settings.
         */
        save(patch) {
            const run = tail.then(async () => {
                await set({ ...current, ...patch });
                current = await get();
                return current;
            });
            tail = run.catch(() => {});
            return run;
        },
        /**
         * Adopts a snapshot obtained outside this sequence (folder
         * picker/clear results, the mount load) so the next patch
         * merges over it instead of clobbering it.
         */
        acknowledge(snapshot) {
            current = snapshot;
        },
    };
}
