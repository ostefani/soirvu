<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    let {
        showOut = false,
        showBack = false,
        // Fully resolved outgoing rows: { id, label, metaTitle,
        // metaDate, metaStatus } with metaStatus null while unknown.
        targets = [],
        resolving = false,
        resolveError = '',
        // Verified backlink rows: { id, title, date, crumb }.
        backRows = [],
        backDone = false,
        checked = 0,
        scanning = false,
        scanStopped = false,
        scanError = '',
        pendingLarge = 0,
        onToggleOut = null,
        onToggleBack = null,
        onRemoveTarget = null,
        onRetryScan = null,
        onMoreScan = null,
        onRefreshScan = null,
        onStopScan = null,
        onOpenEntry = null,
    } = $props();
</script>

<div class="link-panels">
    <div class="panel">
        <button
            class="panel-head"
            aria-expanded={showOut}
            onclick={onToggleOut}>
            <span>{showOut ? '▾' : '▸'} Links · {targets.length}</span>
        </button>
        {#if showOut}
            {#if targets.length === 0}
                <p class="note">No outgoing links yet. Select text and choose Link… to connect this entry.</p>
            {:else}
                <ul class="rows">
                    {#each targets as t (t.id)}
                        <li class="row">
                            <button class="go" onclick={() => onOpenEntry?.(t.id)}>
                                <span class="row-label">{t.label}</span>
                                {#if t.metaStatus === null || resolving}
                                    <span class="row-meta">…</span>
                                {:else if t.metaStatus === 'ok'}
                                    <span class="row-meta">{t.metaTitle} · {t.metaDate}</span>
                                {:else if t.metaStatus === 'deleted'}
                                    <span class="row-meta warn">Entry deleted.</span>
                                {:else}
                                    <span class="row-meta warn">Entry unavailable.</span>
                                {/if}
                            </button>
                            <button
                                class="mini"
                                aria-label={`Remove link ${t.label}`}
                                onclick={() => onRemoveTarget?.(t.id)}>
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
            onclick={onToggleBack}>
            <span>{showBack ? '▾' : '▸'} Linked from{#if backRows.length > 0} · {backRows.length}{/if}</span>
        </button>
        {#if showBack}
            {#if scanError}
                <p class="note" role="status">
                    {scanError}
                    <button class="mini" onclick={onRetryScan}>Retry</button>
                </p>
            {:else if backRows.length === 0 && backDone && !scanning}
                <p class="note">No entries link here yet.</p>
            {:else}
                <ul class="rows">
                    {#each backRows as b (b.id)}
                        <li class="row">
                            <button class="go" onclick={() => onOpenEntry?.(b.id)}>
                                <span class="row-label">{b.title}</span>
                                <span class="row-meta">
                                    {b.date}
                                    {#if b.crumb} · {b.crumb}{/if}
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
                        <button class="mini" onclick={onStopScan}>Stop</button>
                    {:else}
                        <span class="note">
                            {#if scanStopped}Stopped after {checked} {checked === 1 ? 'source' : 'sources'}.
                            {:else if backDone}Checked {checked} {checked === 1 ? 'source' : 'sources'}.
                            {:else}Checked {checked} {checked === 1 ? 'source' : 'sources'} so far.{/if}
                        </span>
                        {#if !backDone && (backRows.length > 0 || scanStopped)}
                            <button class="mini" onclick={onMoreScan}>More…</button>
                        {/if}
                        <button class="mini" onclick={onRefreshScan}>Refresh</button>
                    {/if}
                </div>
            {/if}
        {/if}
    </div>
</div>

<style src="./styles.css"></style>
