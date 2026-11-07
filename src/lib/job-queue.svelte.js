export function createJobQueue({ concurrency, run }) {
    const waiting = [];
    const running = new Set();
    let aborted = false;
    let pending = $state(0);
    let failures = $state.raw([]);

    function add(items) {
        if (pending === 0) failures = [];
        for (const item of items) waiting.push(item);
        pending += items.length;
        drain();
    }

    function drain() {
        if (aborted) return;
        while (running.size < concurrency && waiting.length > 0) {
            start(waiting.shift());
        }
    }

    function start(item) {
        const task = (async () => run(item))()
            .then(
                (failure) => {
                    if (failure) failures = [...failures, failure];
                },
                (error) => {
                    failures = [...failures, String(error)];
                },
            )
            .finally(() => {
                running.delete(task);
                pending--;
                drain();
            });
        running.add(task);
    }

    async function abort() {
        aborted = true;
        pending -= waiting.length;
        waiting.length = 0;
        await Promise.allSettled([...running]);
    }

    function resume() {
        aborted = false;
    }

    async function settle() {
        while (!aborted && (waiting.length > 0 || running.size > 0)) {
            await Promise.allSettled([...running]);
        }
    }

    return {
        add,
        abort,
        resume,
        settle,
        get pending() {
            return pending;
        },
        get failures() {
            return failures;
        },
        get aborted() {
            return aborted;
        },
    };
}
