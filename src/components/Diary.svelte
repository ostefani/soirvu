<script>
    // The unlocked workspace: header, entry list, and the
    // main pane (calendar home, editor, or settings). It
    // exists only while the vault is unlocked — the shell
    // mounts it on unlock and destroys it on lock, so all
    // of this state dies with the component and nothing
    // diary-shaped can outlive the lock. Locking itself is
    // the shell's job: onLock asks it to lock.
    import * as api from '../api.js';
    import EntryList from './EntryList.svelte';
    import Calendar from './Calendar.svelte';
    import EntryEditor from './EntryEditor.svelte';
    import Settings from './Settings.svelte';

    let { vault, onLock } = $props();

    let selectedId = $state(null);
    let listEl = $state(null);
    let showSettings = $state(false);
    let listDate = $state(null);
    let calVersion = $state(0);
    let onThisDay = $state([]);

    async function loadOnThisDay() {
        const now = new Date();
        const pad = (n) => String(n).padStart(2, '0');
        const today = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
        try {
            onThisDay = await api.onThisDay(today);
        } catch {
            onThisDay = [];
        }
    }

    $effect(() => {
        void calVersion;
        loadOnThisDay();
    });
</script>

<div class="diary">
    <header class="diary-head">
        <span class="vault-label">{vault?.name}</span>
        <span>
            <button class="quiet" onclick={() => (showSettings = true)}>Settings</button>
            <button class="quiet" onclick={onLock}>Lock</button>
        </span>
    </header>
    <div class="diary-body">
        <aside>
            <EntryList
                bind:this={listEl}
                {selectedId}
                onSelect={(id) => (selectedId = id)}
                date={listDate}
                onClearDate={() => (listDate = null)} />
        </aside>
        <main>
            {#if showSettings}
                <Settings
                    onBack={() => (showSettings = false)}
                    onImported={() => {
                        listEl?.refresh();
                        calVersion++;
                    }} />
            {:else if selectedId}
                {#key selectedId}
                    <EntryEditor
                        entryId={selectedId}
                        onBack={() => (selectedId = null)}
                        onDeleted={() => {
                            selectedId = null;
                            listEl?.refresh();
                            calVersion++;
                        }}
                        onSaved={() => {
                            listEl?.refresh();
                            calVersion++;
                        }} />
                {/key}
            {:else}
                <div class="calendar-home">
                    <Calendar large selectedDate={listDate} onSelect={(d) => (listDate = d)} version={calVersion} />
                    <p class="note">
                        Pick a day to see its entries in the list — or pick a recent entry on the left.
                    </p>
                    {#if onThisDay.length}
                        <div class="on-this-day">
                            <h3>On this day</h3>
                            {#each onThisDay as entry (entry.id)}
                                <button class="otd-row" onclick={() => (selectedId = entry.id)}>
                                    <span class="otd-year">{entry.entry_date.slice(0, 4)}</span>
                                    <span>{entry.title || 'Untitled'}</span>
                                </button>
                            {/each}
                        </div>
                    {/if}
                </div>
            {/if}
        </main>
    </div>
</div>

<style>
    /* The diary shell: header bar and the two-pane body. */
    .diary {
        display: flex;
        flex-direction: column;
        height: 100vh;
    }

    .diary-head {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 0.5rem 1rem;
        border-bottom: 1px solid rgba(127, 127, 127, 0.35);
    }

    .vault-label {
        font-weight: 600;
    }

    .diary-body {
        display: flex;
        flex: 1;
        min-height: 0;
    }

    .diary-body aside {
        width: 19rem;
        flex-shrink: 0;
        border-right: 1px solid rgba(127, 127, 127, 0.35);
        overflow-y: auto;
    }

    .diary-body main {
        flex: 1;
        overflow-y: auto;
        padding: 1rem 1.5rem;
    }
    .on-this-day {
        width: 100%;
        max-width: 620px;
        margin-top: 1.5rem;
        text-align: left;
    }

    .on-this-day h3 {
        margin: 0 0 0.4rem;
        font-size: 1rem;
    }

    .otd-row {
        display: flex;
        gap: 0.75rem;
        width: 100%;
        text-align: left;
        background: none;
        border: none;
        padding: 0.35rem 0.25rem;
        cursor: pointer;
    }

    .otd-year {
        opacity: 0.55;
        font-variant-numeric: tabular-nums;
    }
</style>
