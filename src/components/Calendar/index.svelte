<script>
    import Button from '@/components/Button/index.svelte';
    import IconLeft from '@/icons/arrow-left.svelte';
    import IconRight from '@/icons/arrow-right.svelte';
    import * as api from '@/api.js';

    let { selectedDate = null, onSelect, version = 0, large = false } = $props();

    const now = new Date();
    let year = $state(now.getFullYear());
    let month = $state(now.getMonth());
    let counts = $state({});

    const MONTH_NAMES = [
        'January',
        'February',
        'March',
        'April',
        'May',
        'June',
        'July',
        'August',
        'September',
        'October',
        'November',
        'December',
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

<div class="calendar">
    <div class="cal-head">
        <Button kind="service-button" handler={() => step(-1)} aria-label="Previous month"><IconLeft /></Button>
        <span class="cal-title">{MONTH_NAMES[month]} {year}</span>
        <Button kind="service-button" handler={() => step(1)} aria-label="Next month"><IconRight /></Button>
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
                    onclick={() => pick(day)}>
                    {day}
                </button>
            {/if}
        {/each}
    </div>
</div>

<style src="./styles.css"></style>
