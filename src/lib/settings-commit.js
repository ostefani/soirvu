// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

/** Serialize preference patches and canonical reads, including refreshes after native pickers. */
export function createSettingsCommit({ set, get }) {
    let tail = Promise.resolve();
    function enqueue(work) {
        const run = tail.then(work);
        tail = run.catch(() => {});
        return run;
    }

    return {
        save(patch) {
            return enqueue(async () => {
                await set(patch);
                return get();
            });
        },
        refresh() {
            return enqueue(get);
        },
    };
}
