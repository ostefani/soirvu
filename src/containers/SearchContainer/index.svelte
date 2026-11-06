<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import Checkbox from '@/components/Checkbox/index.svelte';

    let { query, scopeFolderName, searchScoped, onQueryChange, onScopedChange } = $props();

    // The collapsed field hides the checkbox, so the placeholder says
    // where a search will look.
    // const label = $derived(searchScoped && scopeFolderName ? `Search in ${scopeFolderName}` : 'Search the diary');
    const label = $derived(searchScoped ? `Search in ${scopeFolderName}` : 'Search the diary');
</script>

<!-- Collapsed: a compact field. Focus anywhere inside (input or
     checkbox) expands it into a panel. -->
<div class="search" role="search">
    <div class="input-container">
        <label class="inner-label">
            <svg class="icon" viewBox="0 0 16 16" aria-hidden="true">
                <circle cx="7" cy="7" r="4.75" />
                <path d="M10.6 10.6 14 14" />
            </svg>
            <input
                type="search"
                placeholder="{label}…"
                aria-label={label}
                value={query}
                oninput={(e) => onQueryChange(e.currentTarget.value)} />
        </label>
        {#if scopeFolderName}
            <!-- Safari does not focus a checkbox on click, so a plain
                 mousedown would blur the input and collapse the panel
                 under the pointer. Keep focus where it is. -->
            <div class="search-options" role="presentation" onmousedown={(e) => e.preventDefault()}>
                <div class="options-clip">
                    <div class="option">
                        <Checkbox
                            checked={searchScoped}
                            onchange={(e) => onScopedChange(e.currentTarget.checked)}
                            label={`Only in ${scopeFolderName}`} />
                    </div>
                </div>
            </div>
        {/if}
    </div>
</div>

<style src="./styles.css"></style>
