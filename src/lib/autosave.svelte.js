// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
export function createAutosave({ snapshot, send, delay = 900 }) {
    /** @type {'idle' | 'dirty' | 'saving' | 'saved' | 'error'} */
    let state = $state('idle');

    let revision = 0;
    let saved = 0;
    let timer = null;
    let running = null;
    let takeSnapshot = snapshot; // () => payload | null, must be synchronous

    async function run() {
        while (saved < revision) {
            const rev = revision;
            const payload = takeSnapshot(); // taken just before sending
            if (!payload) return;
            state = 'saving';
            try {
                await send(payload);
                saved = rev;
            } catch {
                state = 'error';
                return; // the next touch() or flush() retries
            }
        }
        state = 'saved';
    }

    /** Settles in-flight saves; resolves whether the draft is
        clean. Awaiting the old void promise could not tell a failed
        save from a sent one — send failures resolve the run
        internally, so callers must read the outcome, not the
        settlement. */
    function flush() {
        clearTimeout(timer);
        running ??= run().finally(() => (running = null));
        return running.then(() => saved === revision);
    }

    /** Call on every edit. */
    function touch() {
        revision++;
        state = 'dirty';
        clearTimeout(timer);
        timer = setTimeout(flush, delay);
    }

    /** Stop the timer without saving. */
    function cancel() {
        clearTimeout(timer);
    }

    /** Treat the current content as saved (the entry is being deleted). */
    function discard() {
        clearTimeout(timer);
        saved = revision;
    }

    /** For teardown: flush the final snapshot, then report
        whether the entry is clean. The caller is already gone
        by the time this settles, so the result — not the state
        — is what a teardown notice reads. */
    function close() {
        if (saved === revision) return running ? running.then(() => saved === revision) : Promise.resolve(true);
        const last = takeSnapshot();
        takeSnapshot = () => last;
        return flush().then(() => saved === revision);
    }

    return {
        get state() {
            return state;
        },
        get clean() {
            return saved === revision;
        },
        touch,
        cancel,
        discard,
        flush,
        close,
    };
}
