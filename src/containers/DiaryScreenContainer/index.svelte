<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy } from 'svelte';
    import * as api from '@/api.js';
    import EntryList from '@/components/EntryList/index.svelte';
    import Calendar from '@/components/Calendar/index.svelte';
    import EntryEditor from '@/components/EntryEditor/index.svelte';
    import Button from '@/components/Button/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';

    let { vault, onLock, onSettings, importTick = 0 } = $props();

    let selectedId = $state(null);
    let listDate = $state(null);
    let navVersion = $state(0);
    let onThisDay = $state([]);

    // Lifted from EntryList: the header owns the search and the
    // creation action, so their state lives next to their inputs.
    let query = $state('');
    let results = $state(null); // null = not searching
    let searchNote = $state('');
    let searchTimer = null;
    let error = $state('');
    let creating = $state(false);

    // Same short texts as the other screens: codes, never prose
    // from below.
    function describeError(e) {
        switch (String(e)) {
            case 'locked':
                return 'The vault is locked.';
            case 'invalid_input':
                return 'That was not accepted (invalid_input).';
            case 'storage':
                return 'A storage problem (storage).';
            default:
                return `Something went wrong (${String(e)}).`;
        }
    }

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

    function bumpNavigation() {
        navVersion++;
    }

    let lastImportTick = 0;

    $effect(() => {
        if (importTick !== lastImportTick) {
            lastImportTick = importTick;
            navVersion++;
        }
    });

    $effect(() => {
        void navVersion;
        loadOnThisDay();
    });

    function today() {
        const d = new Date();
        const pad = (n) => String(n).padStart(2, '0');
        return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    }

    async function newEntry() {
        if (creating) return;
        creating = true;
        try {
            const created = await api.entrySave({
                id: null,
                entry_date: today(),
                title: null,
                body_md: '',
                mood: null,
                tz: Intl.DateTimeFormat().resolvedOptions().timeZone,
                tags: [],
            });
            // A new entry belongs to the plain list, not to whatever
            // the user was searching for a moment ago.
            query = '';
            results = null;
            searchNote = '';
            bumpNavigation();
            selectedId = created.id;
            error = '';
        } catch (e) {
            error = describeError(e);
        } finally {
            creating = false;
        }
    }

    function onSearchInput() {
        clearTimeout(searchTimer);
        searchTimer = setTimeout(async () => {
            const q = query.trim();
            if (!q) {
                results = null;
                searchNote = '';
                return;
            }
            try {
                results = await api.search(q);
                searchNote = results.length === 0 ? 'Nothing found.' : '';
            } catch (e) {
                results = null;
                searchNote =
                    e === 'query_too_short' ? 'Type at least 3 characters (2 for Russian).' : `Search failed (${e}).`;
            }
        }, 250);
    }

    onDestroy(() => clearTimeout(searchTimer));
</script>

<div class="diary-screen">
    <header class="diary-head">
        <span class="vault-label">{vault?.name}</span>
        <div class="diary-menu">
            <input
                type="search"
                placeholder="Search the diary…"
                aria-label="Search the diary"
                bind:value={query}
                oninput={onSearchInput} />
            <Button label="New entry" kind="texture-button" handler={newEntry} disabled={creating} />
            <Button label="Settings" kind="texture-button" handler={onSettings} />
            <Button label="Lock" kind="texture-button" handler={onLock} />
        </div>
        {#if error}<p class="form-error">{error}</p>{/if}
    </header>
    <div class="diary-body">
        <aside>
            <EntryList
                version={navVersion}
                {selectedId}
                onSelect={(id) => (selectedId = id)}
                date={listDate}
                onClearDate={() => (listDate = null)}
                {results}
                {searchNote}
                onClearSearch={() => {
                    query = '';
                    results = null;
                    searchNote = '';
                }} />
        </aside>
        <main>
            {#if selectedId}
                {#key selectedId}
                    <EntryEditor
                        entryId={selectedId}
                        onBack={() => (selectedId = null)}
                        onDeleted={() => {
                            selectedId = null;
                            bumpNavigation();
                        }}
                        onSaved={bumpNavigation} />
                {/key}
            {:else}
                <div class="calendar-home">
                    <Calendar large selectedDate={listDate} onSelect={(d) => (listDate = d)} version={navVersion} />
                    <NoteComponent
                        variant="info"
                        text="Pick a day to see its entries in the list — or pick a recent entry on the left." />
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

<style src="./styles.css"></style>
