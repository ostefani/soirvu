<script>
    // The month at a glance (§13 Phase 6): days with entries
    // carry a dot, today is bold, the day the list is filtered
    // to is ringed. Clicking a day filters the entry list to
    // it; clicking it again clears the filter.
    import * as api from '../api.js';

    let { selectedDate = null, onSelect, version = 0, large = false } = $props();

    const now = new Date();
    let year = $state(now.getFullYear());
    let month = $state(now.getMonth());
    let counts = $state({});

    const MONTH_NAMES = [
        'January', 'February', 'March', 'April', 'May', 'June',
        'July', 'August', 'September', 'October', 'November', 'December',
    ];
    const WEEKDAYS = ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su'];

    const pad = (n) => String(n).padStart(2, '0');
    const iso = (y, m, d) => `${y}-${pad(m + 1)}-${pad(d)}`;
    const todayIso = iso(now.getFullYear(), now.getMonth(), now.getDate());

    async function load() {
        const from = iso(year, month, 1);
        const to = month === 11 ? iso(year + 1, 0, 1) : iso(year, month + 1, 1);
        try {
            const days = await api.calendarDays(from, to);
            counts = Object.fromEntries(days.map((d) => [d.date, d.entries]));
        } catch {
            counts = {};
        }
    }

    $effect(() => {
        void year;
        void month;
        void version;
        load();
    });

    // Monday-first grid: blanks before the 1st, then the days.
    function cells() {
        const first = new Date(year, month, 1);
        const blanks = (first.getDay() + 6) % 7;
        const daysInMonth = new Date(year, month + 1, 0).getDate();
        const out = [];
        for (let i = 0; i < blanks; i++) out.push(null);
        for (let d = 1; d <= daysInMonth; d++) out.push(d);
        return out;
    }

    function step(delta) {
        const next = new Date(year, month + delta, 1);
        year = next.getFullYear();
        month = next.getMonth();
    }

    function pick(day) {
        const date = iso(year, month, day);
        onSelect(date === selectedDate ? null : date);
    }
</script>

<div class="calendar" class:large>
    <div class="cal-head">
        <button class="quiet" onclick={() => step(-1)} aria-label="Previous month">‹</button>
        <span class="cal-title">{MONTH_NAMES[month]} {year}</span>
        <button class="quiet" onclick={() => step(1)} aria-label="Next month">›</button>
    </div>
    <div class="cal-grid">
        {#each WEEKDAYS as name}
            <span class="cal-weekday">{name}</span>
        {/each}
        {#each cells() as day}
            {#if day === null}
                <span></span>
            {:else}
                {@const date = iso(year, month, day)}
                <button
                    class="cal-day"
                    class:has-entries={counts[date]}
                    class:today={date === todayIso}
                    class:selected={date === selectedDate}
                    title={counts[date] ? `${counts[date]} ${counts[date] === 1 ? 'entry' : 'entries'}` : ''}
                    onclick={() => pick(day)}
                >
                    {day}
                </button>
            {/if}
        {/each}
    </div>
</div>

<style>
    .calendar {
        padding: 0.5rem 0.75rem 0.75rem;
    }

    .calendar.large {
        max-width: 620px;
        margin: 0 auto;
        padding: 1rem 1.25rem 1.25rem;
    }

    .calendar.large .cal-title {
        font-size: 1.15rem;
    }

    .calendar.large .cal-day {
        font-size: 1rem;
        padding: 0.55rem 0;
    }

    .calendar.large .cal-weekday {
        font-size: 0.8rem;
    }

    .cal-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 0.35rem;
    }

    .cal-title {
        font-weight: 600;
        font-size: 0.9rem;
    }

    .cal-grid {
        display: grid;
        grid-template-columns: repeat(7, 1fr);
        gap: 2px;
        text-align: center;
    }

    .cal-weekday {
        font-size: 0.7rem;
        opacity: 0.55;
        padding: 0.15rem 0;
    }

    .cal-day {
        position: relative;
        border: none;
        background: none;
        border-radius: 999px;
        padding: 0.22rem 0;
        font-size: 0.82rem;
        cursor: pointer;
        color: inherit;
    }

    .cal-day:hover {
        background: rgba(0, 0, 0, 0.06);
    }

    .cal-day.has-entries::after {
        content: '';
        position: absolute;
        left: 50%;
        bottom: 1px;
        width: 4px;
        height: 4px;
        margin-left: -2px;
        border-radius: 999px;
        background: #396cd8;
    }

    .cal-day.today {
        font-weight: 700;
    }

    .cal-day.selected {
        background: #396cd8;
        color: #fff;
    }

    .cal-day.selected::after {
        background: #fff;
    }
</style>
