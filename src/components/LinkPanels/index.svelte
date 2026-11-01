<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy, untrack } from 'svelte';
    import * as api from '@/api.js';
    import { breadcrumb } from '@/lib/folders.js';
    import { outgoingEntryRefs, verifyBacklinkCandidates } from '@/lib/links.js';

    let {
        entryId,
        rev,
        readBody,
        folders = [],
        onOpenEntry = null,
        onRemoveLink = null,
    } = $props();

    const PAGE = 20;
    const BATCH = 8;

    let showOut = $state(false);
    let showBack = $state(false);
    let refreshed = $state(0);

    // Generations: lock, vault switch and unmount discard late
    // responses instead of rendering them under a new entry.
    let metaRun = 0;
    let scanRun = 0;
    onDestroy(() => {
        metaRun++;
        scanRun++;
    });

    const byId = $derived(new Map(folders.map((f) => [f.id, f])));

    function crumb(folderId) {
        if (folderId == null) return '';
        return breadcrumb(byId, folderId).join(' / ');
    }

    /* ---- Outgoing links: labels and ranges recompute from the
       live body; target metadata resolves in bounded batches on
       expand and after every save. ---- */

    const refs = $derived.by(() => {
        void entryId;
        void rev;
        void refreshed;
        void showOut;
        return outgoingEntryRefs(readBody());
    });

    const distinctTargets = $derived([...new Set(refs.map((r) => r.id))]);

    let targetMeta = $state({});
    let resolving = $state(false);
    let resolveError = $state('');

    async function resolveTargets() {
        const mine = ++metaRun;
        resolving = true;
        resolveError = '';
        try {
            const ids = [...new Set(outgoingEntryRefs(readBody()).map((r) => r.id))];
            const next = {};
            for (let i = 0; i < ids.length; i += BATCH) {
                const chunk = await Promise.all(
                    ids.slice(i, i + BATCH).map(async (id) => {
                        try {
                            const e = await api.entryGet(id);
                            return [id, { status: 'ok', title: e.title, date: e.entry_date }];
                        } catch (e) {
                            return [id, { status: e === 'entry_not_found' ? 'deleted' : 'missing' }];
                        }
                    }),
                );
                if (mine !== metaRun) return;
                for (const [id, meta] of chunk) next[id] = meta;
                targetMeta = next;
            }
        } catch {
            if (mine !== metaRun) return;
            resolveError = 'Could not read the targets.';
        } finally {
            if (mine === metaRun) resolving = false;
        }
    }

    $effect(() => {
        // Rev rides along so a save re-resolves visible metadata.
        // Progress writes below never retrigger this: only rev and
        // the disclosure are tracked.
        void rev;
        if (showOut) resolveTargets();
        return () => {
            metaRun++;
        };
    });

    function labelFor(id) {
        return refs.find((r) => r.id === id)?.label || 'Untitled link';
    }

    function removeTarget(id) {
        // Recompute against the live body: positions stored at
        // expand time may predate keystrokes. Back-to-front keeps
        // the remaining ranges valid as each mark comes out.
        const live = outgoingEntryRefs(readBody())
            .filter((r) => r.id === id)
            .sort((a, b) => b.from - a.from);
        if (!live.length) {
            refreshed++;
            return;
        }
        for (const ref of live) onRemoveLink?.({ from: ref.from, to: ref.to });
        refreshed++;
    }

    /* ---- Backlinks: candidate pages verify one by one until a
       page of verified rows fills or the candidates run out.
       Unverified sources never render. ---- */

    let backlinks = $state([]);
    let backCursor = $state(null);
    let backDone = $state(false);
    let checked = $state(0);
    let scanning = $state(false);
    let scanStopped = $state(false);
    let scanError = $state('');

    async function scanBacklinks(reset) {
        const target = entryId;
        const mine = ++scanRun;
        scanStopped = false;
        if (reset) {
            backlinks = [];
            backCursor = null;
            backDone = false;
            checked = 0;
            scanStopped = false;
        }
        scanning = true;
        scanError = '';
        try {
            let collected = reset ? [] : [...backlinks];
            let cursor = reset ? null : backCursor;
            let count = reset ? 0 : checked;
            let exhausted = reset ? false : backDone;
            const want = collected.length + PAGE;
            while (collected.length < want && !exhausted) {
                const page = await api.backlinkCandidates(target, { cursor, limit: PAGE });
                if (mine !== scanRun) return;
                count += page.candidates.length;
                for (const v of verifyBacklinkCandidates(target, page.candidates)) {
                    if (!collected.some((c) => c.id === v.id)) collected.push(v);
                }
                cursor = page.next_cursor;
                exhausted = !cursor;
                backlinks = collected;
                checked = count;
                backCursor = cursor;
                backDone = exhausted;
            }
        } catch {
            if (mine !== scanRun) return;
            scanError = 'Could not scan for links.';
        } finally {
            if (mine === scanRun) scanning = false;
        }
    }

    function stopScan() {
        scanRun++;
        scanning = false;
        scanStopped = true;
    }

    function toggleOut() {
        showOut = !showOut;
        // Collapsing stops the resolve; the effect cleanup aborts
        // the loop, this resets the visible flag it would leave set.
        if (!showOut && resolving) {
            metaRun++;
            resolving = false;
        }
    }

    function toggleBack() {
        showBack = !showBack;
        if (!showBack && scanning) stopScan();
    }

    // Which entry the auto-scan already served. Plain (not
    // reactive) state on purpose: progress publishes must never
    // retrigger this effect and abort their own scan.
    let autoStartedFor = null;

    $effect(() => {
        const target = entryId;
        const open = showBack;
        // Auto-scan once on expand; Refresh/More… own the rest. A
        // stop or a failure never restarts by itself, and collapsing
        // aborts through the cleanup below.
        if (open && autoStartedFor !== target && untrack(() => backlinks.length === 0 && !backDone)) {
            autoStartedFor = target;
            scanBacklinks(true);
        }
        return () => {
            scanRun++;
        };
    });
</script>

<div class="link-panels">
    <div class="panel">
        <button
            class="panel-head"
            aria-expanded={showOut}
            onclick={toggleOut}>
            <span>{showOut ? '▾' : '▸'} Links · {distinctTargets.length}</span>
        </button>
        {#if showOut}
            {#if refs.length === 0}
                <p class="note">No outgoing links yet. Select text and choose Link… to connect this entry.</p>
            {:else}
                <ul class="rows">
                    {#each distinctTargets as id (id)}
                        {@const meta = targetMeta[id]}
                        <li class="row">
                            <button class="go" onclick={() => onOpenEntry?.(id)}>
                                <span class="row-label">{labelFor(id)}</span>
                                {#if !meta || resolving}
                                    <span class="row-meta">…</span>
                                {:else if meta.status === 'ok'}
                                    <span class="row-meta">{meta.title ?? 'Untitled'} · {meta.date}</span>
                                {:else if meta.status === 'deleted'}
                                    <span class="row-meta warn">Entry deleted.</span>
                                {:else}
                                    <span class="row-meta warn">Entry unavailable.</span>
                                {/if}
                            </button>
                            <button
                                class="mini"
                                aria-label={`Remove link ${labelFor(id)}`}
                                onclick={() => removeTarget(id)}>
                                Remove
                            </button>
                        </li>
                    {/each}
                </ul>
                {#if resolveError}<p class="note" role="status">{resolveError}</p>{/if}
            {/if}
        {/if}
    </div>

    <div class="panel">
        <button
            class="panel-head"
            aria-expanded={showBack}
            onclick={toggleBack}>
            <span>{showBack ? '▾' : '▸'} Linked from{#if backlinks.length > 0} · {backlinks.length}{/if}</span>
        </button>
        {#if showBack}
            {#if scanError}
                <p class="note" role="status">
                    {scanError}
                    <button class="mini" onclick={() => scanBacklinks(true)}>Retry</button>
                </p>
            {:else if backlinks.length === 0 && backDone && !scanning}
                <p class="note">No entries link here yet.</p>
            {:else}
                <ul class="rows">
                    {#each backlinks as b (b.id)}
                        <li class="row">
                            <button class="go" onclick={() => onOpenEntry?.(b.id)}>
                                <span class="row-label">{b.title ?? 'Untitled'}</span>
                                <span class="row-meta">
                                    {b.entry_date}
                                    {#if b.folder_id} · {crumb(b.folder_id)}{/if}
                                </span>
                            </button>
                        </li>
                    {/each}
                </ul>
                <div class="scan-foot">
                    {#if scanning}
                        <span class="note" role="status">Checking {checked} {checked === 1 ? 'source' : 'sources'}…</span>
                        <button class="mini" onclick={stopScan}>Stop</button>
                    {:else}
                        <span class="note">
                            {#if scanStopped}Stopped after {checked} {checked === 1 ? 'source' : 'sources'}.
                            {:else if backDone}Checked {checked} {checked === 1 ? 'source' : 'sources'}.
                            {:else}Checked {checked} {checked === 1 ? 'source' : 'sources'} so far.{/if}
                        </span>
                        {#if !backDone && (backlinks.length > 0 || scanStopped)}
                            <button class="mini" onclick={() => scanBacklinks(false)}>More…</button>
                        {/if}
                        <button class="mini" onclick={() => scanBacklinks(true)}>Refresh</button>
                    {/if}
                </div>
            {/if}
        {/if}
    </div>
</div>

<style src="./styles.css"></style>
