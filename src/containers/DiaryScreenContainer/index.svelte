<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { untrack } from 'svelte';
    import * as api from '@/api.js';
    import { resolveErrorText } from '@/lib/errors.js';
    import { createNavigator } from '@/lib/navigation.js';
    import { clearBacklinkCache } from '@/lib/links.js';
    import EntryList from '@/components/EntryList/index.svelte';
    import FolderTree from '@/components/FolderTree/index.svelte';
    import Calendar from '@/components/Calendar/index.svelte';
    import EntryEditor from '@/components/EntryEditor/index.svelte';
    import Button from '@/components/Button/index.svelte';
    import SearchContainer from '@/containers/SearchContainer/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import { DIARY_ERRORS } from './errors.js';
    import { createFolderScope } from './folder-scope.svelte.js';
    import { createSearch } from './search.svelte.js';
    import { createOnThisDay } from './on-this-day.svelte.js';
    import { todayISO } from './today.js';

    let { vault, onLock, onSettings, importTick = 0 } = $props();

    const folderScope = createFolderScope();
    const search = createSearch(folderScope);
    const onThisDay = createOnThisDay();

    let selectedId = $state(null);
    let listDate = $state(null);
    // Bumped whenever the list, calendar or "on this day" must re-read.
    let navVersion = $state(0);
    // Bumped when a new entry resets the list's own tag/date filters.
    let filterEpoch = $state(0);
    let error = $state('');
    let creating = $state(false);

    // Entry history lives in the navigator: pushes, back, forward,
    // reopening and disposal. Snapshots duplicate the visible filter
    // state on purpose, since that is what history restores. The flags
    // mirror the navigator for the template.
    let canGoBack = $state(false);
    let canGoForward = $state(false);
    const nav = createNavigator({
        onChange: () => {
            canGoBack = nav.canGoBack;
            canGoForward = nav.canGoForward;
        },
    });
    let navRestore = $state.raw(null);
    // bind:this targets, read only from handlers.
    let editorRef = $state.raw(null);
    let mainEl;

    // The New-entry destination shown beside the action.
    const destName = $derived(folderScope.folderName ?? 'Root');

    // A new vault retires everything read for the previous one.
    // The navigator is also cleared on unmount (lock).
    $effect(() => {
        void vault?.id;
        nav.reset();
        navRestore = null;
        search.invalidate();
        folderScope.load();
        return clearBacklinkCache;
    });

    $effect(() => {
        void vault?.id;
        void navVersion;
        onThisDay.load();
    });

    // Workspace epoch (see App): bumped by settings after any import or
    // sync that changed entries, folders or tags. Every read model is
    // reloaded here, never by polling. Drafts in the open editor are
    // untouched: only reads refresh, and conflicts surface on the next
    // save. A tick that predates this mount needs no reload.
    let seenImportTick = untrack(() => importTick);
    $effect(() => {
        if (importTick === seenImportTick) return;
        seenImportTick = importTick;
        untrack(() => {
            bumpNavigation();
            folderScope.load();
            search.refresh();
        });
    });

    $effect(() => () => nav.dispose());

    function bumpNavigation() {
        navVersion++;
    }

    function viewSnapshot(entryId, extras) {
        return {
            entryId,
            scope: folderScope.selection,
            scoped: folderScope.searchScoped,
            query: search.query,
            date: listDate,
            sel: null,
            scroll: 0,
            ...extras,
        };
    }

    // What the coordinator restores on back/forward: the visible state
    // plus the editor selection and scroll position.
    function captureSnapshot() {
        return viewSnapshot(selectedId, {
            sel: editorRef?.navCapture?.() ?? null,
            scroll: mainEl?.scrollTop ?? 0,
        });
    }

    // The remounted editor loads async; settle the scroll after it lands.
    function restoreScroll(id, top) {
        requestAnimationFrame(() =>
            requestAnimationFrame(() => {
                if (selectedId === id) mainEl.scrollTop = top;
            }),
        );
    }

    async function selectEntry(id, { push = false, restore = null, context = null } = {}) {
        if (id === selectedId && !restore) return true;
        // A select that resolves after a newer select began is obsolete:
        // it records and applies nothing. The open entry settles uploads
        // and flushes first; a failed save stays put with its error showing.
        const token = nav.beginSelect();
        const ok = (await editorRef?.flushForNavigation?.()) ?? true;
        if (!ok || !nav.isLiveSelect(token)) return false;

        // Snapshot the entry left before context and selection move on.
        const left = push && selectedId ? captureSnapshot() : null;
        let searching;
        if (context) {
            folderScope.restore(context.scope, context.scoped);
            listDate = context.date;
            searching = search.restore(context.query);
        }
        navRestore = restore;
        selectedId = id;
        if (restore && mainEl) restoreScroll(id, restore.scroll ?? 0);
        if (searching) {
            await searching;
            if (!nav.isLiveSelect(token)) return false;
        }
        if (left) {
            // History holds snapshots with the current entry on top:
            // pushing only the entry left made Back disabled after
            // A -> B and skip B after A -> B -> C.
            nav.pushNavigation(left, viewSnapshot(id));
        }
        return true;
    }

    const open = (id) => selectEntry(id, { push: true });

    async function goHistory(delta) {
        const target = nav.peek(delta);
        if (!target) return;
        const ok = await selectEntry(target.entryId, {
            restore: { sel: target.sel, scroll: target.scroll },
            context: { scope: target.scope, scoped: target.scoped, query: target.query, date: target.date },
        });
        if (ok) nav.arriveAt(target);
    }

    async function newEntry() {
        if (creating) return;
        creating = true;
        try {
            // Files into the selected real folder, or the root in the
            // All/Root views; the destination stays put after creation.
            const created = await api.entryCreate(folderScope.folderId, {
                entry_date: todayISO(),
                title: null,
                body_md: '',
                mood: null,
                tz: Intl.DateTimeFormat().resolvedOptions().timeZone,
                tags: [],
            });
            // A new entry belongs to the plain list, not to whatever the
            // user was searching for. Search, date and tag filters clear;
            // the folder context is kept.
            search.clear();
            listDate = null;
            filterEpoch++;
            bumpNavigation();
            error = '';
            await selectEntry(created.id, { push: true });
            // Filing changes direct-entry counts: refresh the navigator.
            await folderScope.load();
        } catch (e) {
            error = resolveErrorText(DIARY_ERRORS, e).text;
        } finally {
            creating = false;
        }
    }

    async function handleFoldersChanged() {
        await folderScope.load();
        bumpNavigation();
    }

    async function handleDeleted() {
        selectedId = null;
        bumpNavigation();
        // Deleting changes direct-entry counts too.
        await folderScope.load();
    }
</script>

<div class="diary-screen">
    <header class="diary-head">
        <span class="vault-label">{vault?.name}</span>
        <div class="diary-menu">
            <SearchContainer
                query={search.query}
                scopeFolderName={folderScope.folderName}
                searchScoped={folderScope.searchScoped}
                onQueryChange={search.setQuery}
                onScopedChange={folderScope.setSearchScoped} />
            <span class="dest" title="New entries file here">in {destName}</span>
            <Button label="New entry" kind="texture-button" handler={newEntry} disabled={creating} />
            <Button label="Settings" kind="texture-button" handler={onSettings} />
            <Button label="Lock" kind="texture-button" handler={onLock} />
        </div>
        {#if error}<p class="form-error">{error}</p>{/if}
    </header>
    <div class="diary-body">
        <aside>
            <FolderTree
                folders={folderScope.folders}
                selection={folderScope.selection}
                onSelect={folderScope.select}
                onChanged={handleFoldersChanged} />
            <EntryList
                version={navVersion}
                {selectedId}
                onSelect={open}
                date={listDate}
                onClearDate={() => (listDate = null)}
                results={search.results}
                searchNote={search.note}
                scope={folderScope.listScope}
                folders={folderScope.folders}
                {filterEpoch}
                onToggleDescendants={folderScope.toggleDescendants}
                onClearSearch={search.clear} />
        </aside>
        <main bind:this={mainEl}>
            {#if selectedId}
                {#key selectedId}
                    <EntryEditor
                        bind:this={editorRef}
                        entryId={selectedId}
                        folders={folderScope.folders}
                        {navRestore}
                        {canGoBack}
                        {canGoForward}
                        workspaceTick={importTick}
                        onOpenEntry={({ id }) => open(id)}
                        onNavBack={() => goHistory(-1)}
                        onNavForward={() => goHistory(1)}
                        onBack={() => (selectedId = null)}
                        onDeleted={handleDeleted}
                        onSaved={bumpNavigation} />
                {/key}
            {:else}
                <div class="calendar-home">
                    <Calendar
                        large
                        selectedDate={listDate}
                        onSelect={(d) => (listDate = d)}
                        version={navVersion}
                        scope={folderScope.listScope} />
                    <NoteComponent
                        variant="info"
                        text="Pick a day to see its entries in the list — or pick a recent entry on the left." />
                    {#if onThisDay.entries.length}
                        <div class="on-this-day">
                            <h3>On this day <span class="vault-wide">· whole vault</span></h3>
                            {#each onThisDay.entries as entry (entry.id)}
                                <button type="button" class="otd-row" onclick={() => open(entry.id)}>
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
