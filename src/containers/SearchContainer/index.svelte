<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import Checkbox from '@/components/Checkbox/index.svelte';

    let { query, scopeFolderName, searchScoped, onQueryChange, onScopedChange } = $props();
</script>

<div class="search-block">
    <input
        type="search"
        placeholder="Search the diary…"
        aria-label="Search the diary"
        value={query}
        oninput={(e) => onQueryChange(e.currentTarget.value)} />
    {#if scopeFolderName}
        <Checkbox
            checked={searchScoped}
            onchange={(e) => onScopedChange(e.currentTarget.checked)}
            label={`Only in ${scopeFolderName}`} />
    {/if}
</div>

<style src="./styles.css"></style>
