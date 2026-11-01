// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

/**
 * One entry's commit sequence: every revision-bearing mutation
 * (content saves, attachment completions) runs exclusively, in
 * submission order, against the newest committed revision.
 *
 * File reading and chunk transfer stay concurrent — only the
 * revision-sensitive tail (submit with a base, adopt the result)
 * goes through here. Each task learns its base when it starts,
 * not when it is enqueued, so two completions submitted from the
 * same observed revision still chain instead of conflicting: the
 * second runs with the first one's committed revision. A task's
 * rejection never advances the revision and never blocks later
 * tasks; a genuinely foreign write surfaces as the backend's
 * conflict, with the draft preserved.
 *
 * @param {number} initialRev the entry revision the editor opened.
 * @param {(result: any) => number} [revOf] reads the committed
 * revision off a task's result (both entry and attachment
 * responses carry `rev`).
 */
export function createEntryCommit(initialRev, revOf = (result) => result.rev) {
    let committed = initialRev;
    // Every task starts after every earlier task settled; the
    // chain itself never rejects, so one failure cannot stall it.
    let tail = Promise.resolve();

    /**
     * Runs `task` exclusively with the newest committed revision.
     * Resolves with the task's result after adopting its revision.
     * Older responses cannot replace newer state: adoption runs in
     * task order, before the next task starts.
     * @param {(baseRev: number) => Promise<any>} task
     * @returns {Promise<any>} the task's result.
     */
    function commit(task) {
        const run = tail.then(() => task(committed));
        tail = run.catch(() => {});
        return run.then((result) => {
            committed = revOf(result);
            return result;
        });
    }

    return {
        /** The newest revision this sequence committed. */
        get rev() {
            return committed;
        },
        commit,
    };
}
