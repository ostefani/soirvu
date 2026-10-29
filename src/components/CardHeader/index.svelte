<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import Description from '@/components/Description/index.svelte';

    /**
     * Title plus explanatory text for a card.
     *
     * `notes` are regular descriptions, `hints` are the small, subtle ones that
     * follow them. Empty entries are skipped, and the whole header is omitted
     * when there is nothing to show.
     *
     * @type {{
     *   title?: string,
     *   notes?: Array<string | null | undefined>,
     *   hints?: Array<string | null | undefined>,
     * }}
     */
    let { title, notes = [], hints = [] } = $props();

    const visibleNotes = $derived((notes ?? []).filter(Boolean));
    const visibleHints = $derived((hints ?? []).filter(Boolean));
</script>

{#if title || visibleNotes.length || visibleHints.length}
    <div class="card-header">
        {#if title}
            <h3 class="card-title">{title}</h3>
        {/if}
        {#each visibleNotes as note}
            <Description text={note} />
        {/each}
        {#each visibleHints as hint}
            <Description size="small" subtle text={hint} />
        {/each}
    </div>
{/if}

<style src="./styles.css"></style>
