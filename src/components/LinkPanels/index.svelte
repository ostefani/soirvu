<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    let {
        backRows = [],
        backDone = false,
        checked = 0,
        scanning = false,
        scanStopped = false,
        scanError = '',
        pendingLarge = 0,
        onRetryScan = null,
        onMoreScan = null,
        onRefreshScan = null,
        onStopScan = null,
        onOpenEntry = null,
    } = $props();
</script>

<div class="link-panels">
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
</div>

<style src="./styles.css"></style>
