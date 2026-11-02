<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy, untrack } from 'svelte';
    import * as api from '@/api.js';
    import { resolveErrorText } from '@/lib/errors.js';
    import { DIARY_ERRORS } from './errors.js';
    import { createNavigator } from '@/lib/navigation.js';
    import { createRequestOwner } from '@/lib/request-owner.js';
    import { clearBacklinkCache } from '@/lib/links.js';
    import { searchScopeKey, shouldReissueSearch } from '@/lib/scope.js';
    import EntryList from '@/components/EntryList/index.svelte';
    import FolderTree from '@/components/FolderTree/index.svelte';
    import Calendar from '@/components/Calendar/index.svelte';
    import EntryEditor from '@/components/EntryEditor/index.svelte';
    import Button from '@/components/Button/index.svelte';
    import Checkbox from '@/components/Checkbox/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';

    let { vault, onLock, onSettings, importTick = 0 } = $props();

    let selectedId = $state(null);
    let listDate = $state(null);
    let navVersion = $state(0);
    let onThisDay = $state([]);
    // Entry navigation history lives in the coordinator: pushes,
    // replacements, back, forward, reopening and disposal. The
    // flags below mirror it for the template; snapshots duplicate
    // the visible filter state on purpose — that is what history
    // restores. Cleared on unmount (lock) and on vault switch.
    let canGoBack = $state(false);
    let canGoForward = $state(false);
    const nav = createNavigator({
        onChange: () => {
            canGoBack = nav.canGoBack;
            canGoForward = nav.canGoForward;
        },
    });
    let navRestore = $state(null);
    let editorRef = $state(null);
    let mainEl = $state(null);

    // Folder scope for the list, calendar, tags and (optionally)
    // search. Null reads the whole vault; the server defaults
    // missing scopes to All, so only folder/root travel.
    let scopeSel = $state({ kind: 'all' });
    let folders = $state([]);
    let foldersLoaded = $state(false);
    let searchScoped = $state(false);
    let filterEpoch = $state(0);

    // One request owner per read purpose — never a shared global:
    // a new search retires the previous search, not the folder
    // refresh, and disposed owners publish nothing.
    const searchOwner = createRequestOwner();
    const folderOwner = createRequestOwner();
    const dayOwner = createRequestOwner();

    const byId = $derived(new Map(folders.map((f) => [f.id, f])));

    const listScope = $derived(
        scopeSel.kind === 'folder'
            ? { kind: 'folder', id: scopeSel.id, includeDescendants: !!scopeSel.includeDescendants }
            : scopeSel.kind === 'root'
              ? { kind: 'root' }
              : null,
    );

    const scopeFolderName = $derived(
        scopeSel.kind === 'folder' ? (byId.get(scopeSel.id)?.name ?? '…') : null,
    );

    // The New-entry destination shown beside the action.
    const destName = $derived(scopeFolderName ?? 'Root');

    async function loadFolders() {
        const token = folderOwner.begin();
        try {
            const next = await api.folderList();
            if (!folderOwner.current(token)) return;
            folders = next;
            foldersLoaded = true;
        } catch {
            // Keep the last good tree: entry and folder counts may
            // lag after a failed refresh, but the navigator must not
            // vanish underneath the list.
        }
    }

    // A folder deleted elsewhere (or by this screen's tree) drops
    // the scope back to the whole vault instead of erroring.
    $effect(() => {
        if (foldersLoaded && scopeSel.kind === 'folder' && !byId.has(scopeSel.id)) {
            scopeSel = { kind: 'all' };
        }
    });

    $effect(() => {
        if (scopeSel.kind !== 'folder' && searchScoped) searchScoped = false;
    });

    // A visible search follows its scope inputs: checking the box,
    // moving between folders or flipping the subtree flag re-runs
    // it under the new scope instead of leaving stale hits. The
    // query text is read untracked — typing stays on the debounced
    // path and must not fire this — and the key recorded by the
    // search itself keeps its own completion from looping it. An
    // active query reissues even before its first results arrive:
    // the old request retires and the new scope gets its own.
    $effect(() => {
        const key = searchScopeKey(searchScoped, listScope);
        const q = untrack(() => query.trim());
        if (!shouldReissueSearch(key, lastSearchKey, q)) return;
        runSearch(q);
    });

    function handleScopeSelect(sel) {
        scopeSel = sel;
        if (sel.kind !== 'folder') searchScoped = false;
    }

    function toggleDescendants() {
        if (scopeSel.kind !== 'folder') return;
        scopeSel = { ...scopeSel, includeDescendants: !scopeSel.includeDescendants };
    }

    async function handleFoldersChanged() {
        await loadFolders();
        bumpNavigation();
    }

    // Lifted from EntryList: the header owns the search and the
    // creation action, so their state lives next to their inputs.
    let query = $state('');
    let results = $state(null); // null = not searching
    let searchNote = $state('');
    let searchTimer = null;
    let error = $state('');
    let creating = $state(false);

    // Back/forward availability mirrors the coordinator above.

    // Session lifecycle: the first run starts the session's reads;
    // a re-run means the vault changed, so outstanding reads retire
    // and the session restarts. Skipping invalidation on the first
    // run matters: effects run after onMount-style startup, and an
    // unconditional invalidate here would discard the initial
    // folder response and leave the tree empty until another
    // refresh. Beginning the replacement reads in the same tick as
    // the invalidation keeps the switch deterministic either way.
    let sessionStarted = false;
    $effect(() => {
        void vault?.id;
        nav.reset();
        navRestore = null;
        if (sessionStarted) {
            searchOwner.invalidate();
            folderOwner.invalidate();
            dayOwner.invalidate();
            loadFolders();
            loadOnThisDay();
        } else {
            loadFolders();
            sessionStarted = true;
        }
        return () => clearBacklinkCache();
    });

    // Snapshots the visible state the coordinator restores on
    // back/forward. The coordinator owns the stack; this only
    // captures what the UI currently shows.
    function captureSnapshot() {
        return {
            entryId: selectedId,
            scope: scopeSel,
            scoped: searchScoped,
            query,
            date: listDate,
            sel: editorRef?.navCapture?.() ?? null,
            scroll: mainEl?.scrollTop ?? 0,
        };
    }

    // The scope the last search ran under. The effect below
    // re-runs a visible search when the scope inputs move under
    // it — toggling the checkbox, changing folders, flipping the
    // subtree flag — and the key recorded here keeps the search's
    // own completion from re-firing it.
    let lastSearchKey = '';

    async function runSearch(q) {
        const token = searchOwner.begin();
        // Capture the inputs this request runs under before
        // awaiting: the completion checks the token instead of
        // trusting later state, so a late response cannot replace
        // a newer query or scope.
        const scoped = searchScoped && listScope !== null;
        const wireScope = scoped ? listScope : null;
        lastSearchKey = searchScopeKey(searchScoped, listScope);
        try {
            const found = await api.search(q, wireScope ? { scope: wireScope } : {});
            if (!searchOwner.current(token)) return;
            results = found;
            searchNote = found.length === 0 ? 'Nothing found.' : '';
        } catch (e) {
            if (!searchOwner.current(token)) return;
            results = null;
            searchNote =
                e === 'query_too_short'
                    ? 'Type at least 3 characters (2 for Russian).'
                    : `Search failed (${e}).`;
        }
    }

    async function selectEntry(id, opts = {}) {
        if (id === selectedId && !opts.restore) return true;
        // A select that resolves after a newer select began is
        // obsolete: it records and applies nothing. The open entry
        // settles uploads and flushes first; a failed save stays
        // put with its error showing.
        const token = nav.beginSelect();
        const ok = (await editorRef?.flushForNavigation?.()) ?? true;
        if (!ok || !nav.isLiveSelect(token)) return false;
        if (opts.push && selectedId) nav.push(captureSnapshot());
        if (opts.context) {
            scopeSel = opts.context.scope;
            searchScoped = opts.context.scoped && opts.context.scope.kind === 'folder';
            query = opts.context.query;
            listDate = opts.context.date;
        }
        navRestore = opts.restore ?? null;
        selectedId = id;
        if (opts.context && opts.context.query) {
            clearTimeout(searchTimer);
            await runSearch(opts.context.query);
            if (!nav.isLiveSelect(token)) return false;
        }
        if (navRestore && mainEl) {
            // The remounted editor loads async; settle the scroll
            // after it lands.
            const scroll = navRestore.scroll ?? 0;
            requestAnimationFrame(() =>
                requestAnimationFrame(() => {
                    if (selectedId === id) mainEl.scrollTop = scroll;
                }),
            );
        }
        return true;
    }

    async function goHistory(delta) {
        const target = nav.peek(delta);
        if (!target) return;
        const ok = await selectEntry(target.entryId, {
            restore: { sel: target.sel, scroll: target.scroll },
            context: {
                scope: target.scope,
                scoped: target.scoped,
                query: target.query,
                date: target.date,
            },
        });
        if (ok) nav.arriveAt(target);
    }

    // Screen copy comes from the local table; unknown codes use the
    // single shared rule instead of leaking the raw IPC code.
    function describeError(e) {
        return resolveErrorText(DIARY_ERRORS, e).text;
    }

    async function loadOnThisDay() {
        const token = dayOwner.begin();
        const now = new Date();
        const pad = (n) => String(n).padStart(2, '0');
        const today = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
        try {
            const days = await api.onThisDay(today);
            if (!dayOwner.current(token)) return;
            onThisDay = days;
        } catch {
            if (!dayOwner.current(token)) return;
            onThisDay = [];
        }
    }

    function bumpNavigation() {
        navVersion++;
    }

    // Workspace epoch (see App): bumped by settings after any
    // import or sync that changed entries, folders or tags —
    // folder-only changes count. The diary reloads every read
    // model that can show those changes — tree, list, calendar,
    // calendar-day, tags and the visible search — in this one
    // place, never by polling. Drafts in the open editor are
    // untouched: reconciliation only refreshes reads, never the
    // loaded body (conflicts surface on the next save instead).
    let lastImportTick = 0;

    $effect(() => {
        if (importTick !== lastImportTick) {
            lastImportTick = importTick;
            navVersion++;
            loadFolders();
            const q = untrack(() => query.trim());
            if (q) runSearch(q);
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
            // Files into the selected real folder, or the root in
            // the All/Root views: the destination shows beside the
            // action and stays put after creation.
            const folderId = scopeSel.kind === 'folder' ? scopeSel.id : null;
            const created = await api.entryCreate(folderId, {
                entry_date: today(),
                title: null,
                body_md: '',
                mood: null,
                tz: Intl.DateTimeFormat().resolvedOptions().timeZone,
                tags: [],
            });
            // A new entry belongs to the plain list, not to whatever
            // the user was searching for a moment ago. Search, date
            // and tag filters clear; the folder context is kept.
            query = '';
            searchOwner.invalidate();
            results = null;
            searchNote = '';
            listDate = null;
            filterEpoch++;
            bumpNavigation();
            error = '';
            await selectEntry(created.id, { push: true });
            // Filing changes direct-entry counts: refresh the
            // navigator or its badges lag the new entry.
            await loadFolders();
        } catch (e) {
            error = describeError(e);
        } finally {
            creating = false;
        }
    }

    function onSearchInput() {
        clearTimeout(searchTimer);
        // New intent retires the in-flight search immediately —
        // before this keystroke's debounce expires.
        searchOwner.invalidate();
        searchTimer = setTimeout(async () => {
            const q = query.trim();
            if (!q) {
                results = null;
                searchNote = '';
                return;
            }
            // Search defaults to the whole vault; the checkbox
            // narrows it to what the list currently shows.
            await runSearch(q);
        }, 250);
    }

    onDestroy(() => {
        clearTimeout(searchTimer);
        searchOwner.dispose();
        folderOwner.dispose();
        dayOwner.dispose();
        nav.dispose();
    });
</script>

<div class="diary-screen">
    <header class="diary-head">
        <span class="vault-label">{vault?.name}</span>
        <div class="diary-menu">
            <div class="search-block">
                <input
                    type="search"
                    placeholder="Search the diary…"
                    aria-label="Search the diary"
                    bind:value={query}
                    oninput={onSearchInput} />
                {#if scopeFolderName}
                    <Checkbox
                        checked={searchScoped}
                        onchange={(e) => (searchScoped = e.currentTarget.checked)}
                        label={`Only in ${scopeFolderName}`} />
                {/if}
            </div>
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
                {folders}
                selection={scopeSel}
                onSelect={handleScopeSelect}
                onChanged={handleFoldersChanged} />
            <EntryList
                version={navVersion}
                {selectedId}
                onSelect={(id) => selectEntry(id, { push: true })}
                date={listDate}
                onClearDate={() => (listDate = null)}
                {results}
                {searchNote}
                scope={listScope}
                {folders}
                {filterEpoch}
                onToggleDescendants={toggleDescendants}
                onClearSearch={() => {
                    // A late response must not repopulate the
                    // search the list just cleared.
                    searchOwner.invalidate();
                    query = '';
                    results = null;
                    searchNote = '';
                }} />
        </aside>
        <main bind:this={mainEl}>
            {#if selectedId}
                {#key selectedId}
                    <EntryEditor
                        bind:this={editorRef}
                        entryId={selectedId}
                        {folders}
                        {navRestore}
                        {canGoBack}
                        {canGoForward}
                        workspaceTick={importTick}
                        onOpenEntry={({ id }) => selectEntry(id, { push: true })}
                        onNavBack={() => goHistory(-1)}
                        onNavForward={() => goHistory(1)}
                        onBack={() => (selectedId = null)}
                        onDeleted={async () => {
                            selectedId = null;
                            bumpNavigation();
                            // Deleting changes direct-entry counts too.
                            await loadFolders();
                        }}
                        onSaved={bumpNavigation} />
                {/key}
            {:else}
                <div class="calendar-home">
                    <Calendar
                        large
                        selectedDate={listDate}
                        onSelect={(d) => (listDate = d)}
                        version={navVersion}
                        scope={listScope} />
                    <NoteComponent
                        variant="info"
                        text="Pick a day to see its entries in the list — or pick a recent entry on the left." />
                    {#if onThisDay.length}
                        <div class="on-this-day">
                            <h3>On this day <span class="vault-wide">· whole vault</span></h3>
                            {#each onThisDay as entry (entry.id)}
                                <button class="otd-row" onclick={() => selectEntry(entry.id, { push: true })}>
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
