<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->

<script>
    import Checkbox from '@/components/Checkbox/index.svelte';
    let { scopeFolderName, searchScoped, onSearchInput, query, onCheckboxChange } = $props();
    let isFocused = $state(false);

    function onFocus() {
        isFocused = true;
    }

    function onUnFocus() {
        isFocused = false;
    }

    $effect(() => {
        if (isFocused) {
        }
    });
</script>

<div class="search-block" class:isFocused>
    <input
        type="search"
        placeholder="Search the diary…"
        aria-label="Search the diary"
        value={query}
        onfocus={onFocus}
        onfocusout={onUnFocus}
        oninput={onSearchInput} />
    {#if scopeFolderName}
        <Checkbox checked={searchScoped} onchange={onCheckboxChange} label={`Only in ${scopeFolderName}`} />
    {/if}
</div>

<style src="./styles.css"></style>
