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

    function flush() {
        clearTimeout(timer);
        running ??= run().finally(() => (running = null));
        return running;
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

    /** For teardown: freeze the final snapshot, then flush it. */
    function close() {
        if (saved === revision) return running ?? Promise.resolve();
        const last = takeSnapshot();
        takeSnapshot = () => last;
        return flush();
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
