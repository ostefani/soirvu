<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import PaneDivider from '@/components/PaneDivider/index.svelte';
    import { MAIN_MIN, DIVIDERS_WIDTH, PANE_MIN, createModel, paneWidths } from './constants.js';
    import { PANE_LABELS } from './data.js';

    let { folders, entries, children, entryLabel = 'Entries' } = $props();
    let model = $state(createModel());
    let animating = $state(false);
    const widths = $derived(paneWidths(model));

    function observeWorkspace(workspace) {
        const observer = new ResizeObserver(([entry]) => {
            model.width = entry.contentRect.width;
        });
        observer.observe(workspace);
        return () => observer.disconnect();
    }

    function maximum(pane) {
        const other = pane === 'folders' ? 'entries' : 'folders';
        return Math.max(PANE_MIN[pane], model.width - widths[other] - MAIN_MIN - DIVIDERS_WIDTH);
    }

    function finishAnimation(event) {
        if (event.target === event.currentTarget && event.propertyName === 'grid-template-columns') animating = false;
    }

    function togglePane(pane) {
        animating = true;
        model[pane].collapsed = !model[pane].collapsed;
    }

    function resize(pane, width) {
        animating = false;
        model[pane].width = width;
    }
</script>

<div
    class="workspace"
    class:animating
    ontransitionend={finishAnimation}
    {@attach observeWorkspace}
    style:--folders-width={`${widths.folders}px`}
    style:--entries-width={`${widths.entries}px`}>
    {#each ['folders', 'entries'] as pane}
        <section class="navigation-pane" class:collapsed={model[pane].collapsed} aria-label={PANE_LABELS[pane]}>
            <button
                class="pane-toggle"
                aria-label={`${model[pane].collapsed ? 'Expand' : 'Collapse'} ${PANE_LABELS[pane].toLowerCase()} pane`}
                aria-expanded={!model[pane].collapsed}
                aria-controls={`workspace-${pane}`}
                onclick={() => togglePane(pane)}>
                <span title={pane === 'entries' ? entryLabel : PANE_LABELS[pane]}>{pane === 'entries' ? entryLabel : PANE_LABELS[pane]}</span>
                <span aria-hidden="true">{model[pane].collapsed ? '›' : '‹'}</span>
            </button>
            <div class="pane-content" id={`workspace-${pane}`} inert={model[pane].collapsed} aria-hidden={model[pane].collapsed}>
                {#if pane === 'folders'}
                    {@render folders()}
                {:else}
                    {@render entries()}
                {/if}
            </div>
        </section>
        {#if model[pane].collapsed}
            <div class="collapsed-divider"></div>
        {:else}
            <PaneDivider
                label={`Resize ${PANE_LABELS[pane].toLowerCase()} pane`}
                controls={`workspace-${pane}`}
                value={widths[pane]}
                min={PANE_MIN[pane]}
                max={maximum(pane)}
                onResize={(width) => resize(pane, width)} />
        {/if}
    {/each}
    <div class="workspace-main">{@render children()}</div>
</div>

<style src="./styles.css"></style>
