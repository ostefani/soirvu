<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    /**
     * Read-only path display: a label, the value in a monospace box, and
     * optional controls (children) underneath. An empty value shows `emptyText`
     * in the UI font instead.
     *
     * @type {{
     *   label: string,
     *   value?: string | null,
     *   emptyText?: string,
     *   children?: import('svelte').Snippet,
     * }}
     */
    let { label, value, emptyText = '—', children } = $props();
</script>

<div class="location-field">
    <span class="location-label">{label}</span>
    {#if value}
        <code class="location-value">{value}</code>
    {:else}
        <span class="location-value is-empty"><span class="muted">{emptyText}</span></span>
    {/if}
    {@render children?.()}
</div>

<style src="./styles.css"></style>
