<script>
    import * as api from '../../api.js';
    import EntryList from '../EntryList/EntryList.svelte';
    import Calendar from '../Calendar/Calendar.svelte';
    import EntryEditor from '../EntryEditor/EntryEditor.svelte';
    import Settings from '../Settings/Settings.svelte';

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
<style src="./Diary.css"></style>

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
                    <p class="note">Pick a day to see its entries in the list — or pick a recent entry on the left.</p>
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
