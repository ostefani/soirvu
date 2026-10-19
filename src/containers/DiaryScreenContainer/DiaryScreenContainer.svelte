<script>
    import * as api from '../../api.js';
    import EntryList from '../../components/EntryList/EntryList.svelte';
    import Calendar from '../../components/Calendar/Calendar.svelte';
    import EntryEditor from '../../components/EntryEditor/EntryEditor.svelte';
    import Button from '../../components/Button/Button.svelte';

    let { vault, onLock, onSettings } = $props();

    let selectedId = $state(null);
    let listEl = $state(null);
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

    function refreshNavigation() {
        listEl?.refresh();
        calVersion++;
    }

    $effect(() => {
        void calVersion;
        loadOnThisDay();
    });
</script>
<style src="./DiaryScreenContainer.css"></style>

<div class="diary-screen">
    <header class="diary-head">
        <span class="vault-label">{vault?.name}</span>
        <span>
            <Button label="Settings" kind="quiet" handler={onSettings} />
            <Button label="Lock" kind="quiet" handler={onLock} />
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
            {#if selectedId}
                {#key selectedId}
                    <EntryEditor
                        entryId={selectedId}
                        onBack={() => (selectedId = null)}
                        onDeleted={() => {
                            selectedId = null;
                            refreshNavigation();
                        }}
                        onSaved={refreshNavigation} />
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
