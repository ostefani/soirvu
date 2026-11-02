<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy, untrack } from 'svelte';
    import * as api from '@/api.js';
    import { breadcrumb } from '@/lib/folders.js';
    import { isMissingEntryError, outgoingEntryRefs, shouldYield, verifyBacklinkCandidates } from '@/lib/links.js';
    import LinkPanels from '@/components/LinkPanels/index.svelte';

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

    function labelFor(id) {
        return refs.find((r) => r.id === id)?.label || 'Untitled link';
    }

    // Fully resolved rows for the view: no link parsing there.
    const targets = $derived(
        distinctTargets.map((id) => {
            const meta = targetMeta[id];
            return {
                id,
                label: labelFor(id),
                metaTitle: meta?.status === 'ok' ? (meta.title ?? 'Untitled') : null,
                metaDate: meta?.status === 'ok' ? meta.date : null,
                metaStatus: meta ? meta.status : null,
            };
        }),
    );

    let targetMeta = $state({});
    let resolving = $state(false);
    let resolveError = $state('');

    // Takes the already-computed target ids when the caller holds a
    // current analysis (expand, post-save); parses fresh when the
    // body may have moved underneath the display (workspace tick).
    async function resolveTargets(ids = null) {
        const mine = ++metaRun;
        resolving = true;
        resolveError = '';
        try {
            const list = ids ?? [...new Set(outgoingEntryRefs(readBody()).map((r) => r.id))];
            const next = {};
            for (let i = 0; i < list.length; i += BATCH) {
                const chunk = await Promise.all(
                    list.slice(i, i + BATCH).map(async (id) => {
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
        // the disclosure are tracked. Derived analyses flush
        // before effects, so distinctTargets already reflects the
        // saved body and no second parse is needed. Read untracked:
        // subscribing would re-resolve on every removeTarget
        // refresh, which the previous shape never did.
        void rev;
        if (showOut) resolveTargets(untrack(() => distinctTargets));
        return () => {
            metaRun++;
        };
    });

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

    // Display rows for the view: crumbs resolved here.
    const backRows = $derived(
        backlinks.map((b) => ({
            id: b.id,
            title: b.title ?? 'Untitled',
            date: b.entry_date,
            crumb: crumb(b.folder_id),
        })),
    );

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

<LinkPanels
    {showOut}
    {showBack}
    {targets}
    {resolving}
    {resolveError}
    {backRows}
    {backDone}
    {checked}
    {scanning}
    {scanStopped}
    {scanError}
    {pendingLarge}
    onToggleOut={toggleOut}
    onToggleBack={toggleBack}
    onRemoveTarget={removeTarget}
    onRetryScan={() => scanBacklinks(true)}
    onMoreScan={() => scanBacklinks(false)}
    onRefreshScan={() => scanBacklinks(true)}
    onStopScan={stopScan}
    {onOpenEntry} />
