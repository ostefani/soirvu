<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy, untrack } from 'svelte';
    import * as api from '@/api.js';
    import { breadcrumb } from '@/lib/folders.js';
    import { isMissingEntryError, shouldYield, verifyBacklinkCandidates } from '@/lib/links.js';
    import LinkPanels from '@/components/LinkPanels/index.svelte';

    let {
        entryId,
        showBack = false,
        folders = [],
        // Workspace epoch (import/sync changes) from the diary:
        // refreshes visible panels without touching the draft.
        workspaceTick = 0,
        onOpenEntry = null,
    } = $props();

    const PAGE = 20;

    let scanRun = 0;
    onDestroy(() => {
        scanRun++;
    });

    const byId = $derived(new Map(folders.map((f) => [f.id, f])));

    function crumb(folderId) {
        if (folderId == null) return '';
        return breadcrumb(byId, folderId).join(' / ');
    }

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
            stopScan();
        };
    });

    let servedBackTick = 0;
    $effect(() => {
        const tick = workspaceTick;
        const backOpen = untrack(() => autoStartedFor === entryId && showBack);
        if (!backOpen) return;
        servedBackTick = tick;
        scanBacklinks(true);
    });
</script>

<LinkPanels
    {backRows}
    {backDone}
    {checked}
    {scanning}
    {scanStopped}
    {scanError}
    {pendingLarge}
    onRetryScan={() => scanBacklinks(true)}
    onMoreScan={() => scanBacklinks(false)}
    onRefreshScan={() => scanBacklinks(true)}
    onStopScan={stopScan}
    {onOpenEntry} />
