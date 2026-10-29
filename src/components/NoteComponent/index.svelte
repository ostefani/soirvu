<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    let {
        text = '',
        variant = 'neutral', // 'neutral' | 'info' | 'warning' | 'danger'
        title = '',
        icon = true,
        dismissible = false,
        onDismiss = null,
        children = null,
    } = $props();

    let dismissed = $state(false);

    // Auto-detect warning if variant is neutral and text contains warning indicators
    const computedVariant = $derived.by(() => {
        if (variant && variant !== 'neutral') return variant;
        const textContent = Array.isArray(text) ? text.join(' ') : String(text || '');
        if (
            textContent.includes('warning') ||
            textContent.includes('cloud-synced') ||
            textContent.includes('network share') ||
            textContent.includes('risk') ||
            textContent.includes('corruption') ||
            textContent.includes('recovery key')
        ) {
            return 'warning';
        }
        return 'neutral';
    });

    const paragraphs = $derived(
        Array.isArray(text)
            ? text.filter(Boolean)
            : (text ? [text] : [])
    );

    const hasContent = $derived(paragraphs.length > 0 || Boolean(children));

    function handleDismiss() {
        dismissed = true;
        onDismiss?.();
    }
</script>

{#if !dismissed && hasContent}
    <aside
        class="note-callout"
        class:variant-neutral={computedVariant === 'neutral'}
        class:variant-info={computedVariant === 'info'}
        class:variant-warning={computedVariant === 'warning'}
        class:variant-danger={computedVariant === 'danger'}
        role="note">
        {#if icon}
            <span class="note-icon" aria-hidden="true">
                {#if computedVariant === 'warning'}
                    <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                        <path d="M8.982 1.566a1.13 1.13 0 0 0-1.964 0L.165 13.233c-.457.778.091 1.767.982 1.767h13.706c.89 0 1.438-.99.982-1.767L8.982 1.566zM8 5c.535 0 .954.462.9.995l-.35 3.507a.552.552 0 0 1-1.1 0L7.1 5.995A.905.905 0 0 1 8 5zm.002 6a1 1 0 1 1 0 2 1 1 0 0 1 0-2z"/>
                    </svg>
                {:else if computedVariant === 'danger'}
                    <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                        <path d="M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14zm0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16z"/>
                        <path d="M7.002 11a1 1 0 1 1 2 0 1 1 0 0 1-2 0zM7.1 4.995a.905.905 0 1 1 1.8 0l-.35 3.507a.552.552 0 0 1-1.1 0L7.1 4.995z"/>
                    </svg>
                {:else if computedVariant === 'info'}
                    <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                        <path d="M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14zm0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16z"/>
                        <path d="m8.93 6.588-2.29.287-.082.38.45.083c.294.07.352.176.288.469l-.738 3.468c-.194.897.105 1.319.808 1.319.545 0 1.178-.252 1.465-.598l.088-.416c-.2.176-.492.246-.686.246-.275 0-.375-.193-.304-.533L8.93 6.588zM9 4.5a1 1 0 1 1-2 0 1 1 0 0 1 2 0z"/>
                    </svg>
                {:else}
                    <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                        <path d="M8 16A8 8 0 1 0 8 0a8 8 0 0 0 0 16zm.93-9.412-1 4.705c-.07.34.029.533.304.533.194 0 .487-.07.686-.246l-.088.416c-.287.346-.92.598-1.465.598-.704 0-1.002-.422-.808-1.319l.738-3.468c.064-.293.006-.399-.287-.47l-.451-.081.082-.381 2.29-.287zM8 5.5a1 1 0 1 1 0-2 1 1 0 0 1 0 2z"/>
                    </svg>
                {/if}
            </span>
        {/if}

        <div class="note-body">
            {#if title}
                <strong class="note-title">{title}</strong>
            {/if}

            {#if children}
                <div class="note-content">
                    {@render children()}
                </div>
            {:else}
                <div class="note-content">
                    {#each paragraphs as paragraph}
                        <p>{paragraph}</p>
                    {/each}
                </div>
            {/if}
        </div>

        {#if dismissible}
            <button
                class="note-dismiss-button"
                type="button"
                aria-label="Dismiss note"
                onclick={handleDismiss}>
                ×
            </button>
        {/if}
    </aside>
{/if}

<style src="./styles.css"></style>
