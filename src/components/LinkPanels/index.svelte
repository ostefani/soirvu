<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy, untrack } from 'svelte';
    import * as api from '@/api.js';
    import { breadcrumb } from '@/lib/folders.js';
    import { isMissingEntryError, outgoingEntryRefs, shouldYield, verifyBacklinkCandidates } from '@/lib/links.js';

    let {
        entryId,
        rev,
        readBody,
        folders = [],
        // Workspace epoch (import/sync changes) from the diary:
        // refreshes visible panels without touching the draft.
        workspaceTick = 0,
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

    /* ---- Backlinks: candidates verify one by one, in page
       order, until a page of verified rows fills or the
       candidates run out. Unverified sources never render, and
       oversized rows never reorder past newer ones. Oversized
       sources travel as metadata and load whole one at a time,
       so peak memory is one large document instead of a page of
       them; only a confirmed-missing source skips silently. ---- */

    let backlinks = $state([]);
    let backCursor = $state(null);
    let backDone = $state(false);
    let checked = $state(0);
    let scanning = $state(false);
    let scanStopped = $state(false);
    let scanError = $state('');
    // Large bodies currently fetching whole (0 or 1: sequential).
    let pendingLarge = $state(0);
    // Last slice yield: the loop gives the event loop a turn
    // whenever a slice budget elapses between documents.
    let lastYield = 0;

    async function yieldSlice() {
        const now = performance.now();
        if (shouldYield(now, lastYield, 8)) {
            lastYield = now;
            await new Promise((r) => setTimeout(r, 0));
        }
    }

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
            pendingLarge = 0;
        }
        scanning = true;
        scanError = '';
        lastYield = performance.now();
        try {
            let collected = reset ? [] : [...backlinks];
            let cursor = reset ? null : backCursor;
            let count = reset ? 0 : checked;
            let exhausted = reset ? false : backDone;
            const want = collected.length + PAGE;
            while (collected.length < want && !exhausted) {
                const page = await api.backlinkCandidates(target, { cursor, limit: PAGE });
                if (mine !== scanRun) return;
                // Candidate order is the display order: verifying
                // and publishing one by one keeps newer large
                // sources ahead of older small ones, and yields
                // between documents so stop stays responsive.
                for (const c of page.candidates) {
                    if (mine !== scanRun) return;
                    count++;
                    const { verified, pending } = verifyBacklinkCandidates(target, [c]);
                    for (const v of verified) {
                        if (!collected.some((existing) => existing.id === v.id)) collected.push(v);
                    }
                    // Oversized sources arrived as metadata: fetch each
                    // whole body alone, verify it through the same
                    // path (which also caches the verdict), and skip
                    // only sources that vanished mid-scan.
                    for (const p of pending) {
                        if (mine !== scanRun) return;
                        if (collected.some((existing) => existing.id === p.id)) continue;
                        pendingLarge++;
                        try {
                            const full = await api.entryGet(p.id);
                            if (mine !== scanRun) return;
                            // Cache under the fetched body's own hash:
                            // an edit between the metadata read and
                            // this fetch describes different content.
                            const retry = verifyBacklinkCandidates(target, [
                                {
                                    ...p,
                                    body: full.body_md ?? '',
                                    body_hash: full.body_hash ?? p.body_hash,
                                },
                            ]);
                            for (const v of retry.verified) {
                                if (!collected.some((existing) => existing.id === v.id))
                                    collected.push(v);
                            }
                            backlinks = collected;
                        } catch (e) {
                            // Any other failure aborts the scan so it
                            // never reports completion over unread
                            // sources; the cursor keeps the resume
                            // point and backDone stays false.
                            if (!isMissingEntryError(e)) throw e;
                        } finally {
                            // A reset or stop owns the counter from here;
                            // only the live scan decrements it.
                            if (mine === scanRun) pendingLarge--;
                        }
                    }
                    backlinks = collected;
                    checked = count;
                    await yieldSlice();
                }
                cursor = page.next_cursor;
                exhausted = !cursor;
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
        pendingLarge = 0;
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
        if (!open) return;
        // The workspace identity is read untracked on purpose:
        // tracking it would re-run this effect on every tick while
        // open, and the cleanup below would abort the tick effect's
        // own scan. Ticks while open belong to that effect; ticks
        // while collapsed surface as a gap on reopen.
        const tick = untrack(() => workspaceTick);
        if (autoStartedFor !== target) {
            // First expand for this entry: the single lazy scan,
            // reading whatever the workspace holds right now.
            autoStartedFor = target;
            servedBackTick = tick;
            if (untrack(() => backlinks.length === 0 && !backDone)) scanBacklinks(true);
        } else if (untrack(() => servedBackTick) !== tick) {
            // Reopening after a workspace tick that arrived while
            // collapsed: the tick effect below left it unconsumed,
            // so the reopen catches up now instead of fetching for
            // a hidden panel earlier.
            servedBackTick = tick;
            scanBacklinks(true);
        }
        return () => {
            scanRun++;
        };
    });

    // Workspace changes (import/sync) may add or remove links to
    // the open entry. A tick that arrives while both panels are
    // collapsed is not consumed: reopening rescans lazily through
    // the effect above (backlinks) or re-resolves through the rev
    // effect (outgoing metadata). Each panel tracks its own epoch:
    // an outgoing refresh must not mark collapsed backlinks served.
    let servedBackTick = 0;
    $effect(() => {
        const tick = workspaceTick;
        const backOpen = untrack(() => autoStartedFor === entryId && showBack);
        const outOpen = untrack(() => showOut);
        if (!backOpen && !outOpen) return;
        if (backOpen) {
            servedBackTick = tick;
            scanBacklinks(true);
        }
        if (outOpen) resolveTargets();
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
                        <span class="note" role="status"
                            >Checking {checked} {checked === 1 ? 'source' : 'sources'}…{#if pendingLarge > 0}
                                Verifying a large entry…{/if}</span
                        >
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
