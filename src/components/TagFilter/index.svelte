<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    let { tags = [], selected = null, note = '', removing = null, onSelect, onRemove } = $props();
    let tagsOpen = $state(false);
    let confirmRemove = $state(null);

    async function remove(name) {
        if (await onRemove?.(name)) confirmRemove = null;
    }
</script>

<div class="tags-block">
    <button class="tags-toggle" onclick={() => (tagsOpen = !tagsOpen)} aria-expanded={tagsOpen}>
        <span class="tags-toggle-chevron">{tagsOpen ? '▾' : '▸'}</span>
        <span class="tags-toggle-label">Tags</span>
        <span class="tags-toggle-count">{tags.length}</span>
    </button>
    {#if tagsOpen}
        <div class="tag-flow">
            {#each tags as t (t.name)}
                {#if confirmRemove === t.name}
                    <div class="tag-confirm">
                        <span class="confirm-message">Remove “{t.name}” from all {t.entries} entries?</span>
                        <div class="confirm-actions">
                            <button class="confirm-action remove" disabled={removing !== null} onclick={() => remove(t.name)}
                                >Remove</button>
                            <button class="confirm-action cancel" onclick={() => (confirmRemove = null)}
                                >Cancel</button>
                        </div>
                    </div>
                {:else}
                    <div class="tag-pill" class:active={selected === t.name}>
                        <button
                            class="tag-button"
                            class:active={selected === t.name}
                            title={selected === t.name ? `Clear filter: ${t.name}` : `Filter by: ${t.name}`}
                            onclick={() => onSelect?.(t.name)}>
                            <span class="tag-name">{t.name}</span>
                            <span class="tag-count">{t.entries}</span>
                        </button>
                        <button
                            class="tag-remove"
                            title="Remove this tag from all entries"
                            aria-label="Remove tag {t.name}"
                            onclick={() => (confirmRemove = t.name)}>
                            ×
                        </button>
                    </div>
                {/if}
            {/each}
        </div>
        {#if note}<p class="note">{note}</p>{/if}
    {/if}
    {#if !tagsOpen && note}<p class="note" role="status">{note}</p>{/if}
</div>

<style src="./styles.css"></style>
