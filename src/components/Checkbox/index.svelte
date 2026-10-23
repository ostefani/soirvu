<script>
    let {
        checked = false,
        label = '',
        disabled = false,
        name = '',
        id = '',
        color = '',
        gradient = '',
        onchange = null,
        children = null,
    } = $props();

    const customFillStyle = $derived.by(() => {
        if (!checked) return '';
        if (gradient) return `background: ${gradient}; border-color: transparent;`;
        if (color) return `background-color: ${color}; border-color: ${color};`;
        return '';
    });
</script>

<label class="checkbox-container" class:disabled for={id || undefined}>
    <input type="checkbox" class="native-checkbox" {id} {name} {disabled} {checked} {onchange} />
    <span class="checkbox-control" class:checked style={customFillStyle} aria-hidden="true">
        {#if checked}
            <svg
                class="check-icon"
                viewBox="0 0 12 10"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round">
                <polyline points="1.5 5 4.5 8 10.5 2"></polyline>
            </svg>
        {/if}
    </span>

    {#if children}
        <span class="checkbox-label">
            {@render children()}
        </span>
    {:else if label}
        <span class="checkbox-label">{label}</span>
    {/if}
</label>

<style src="./styles.css"></style>
