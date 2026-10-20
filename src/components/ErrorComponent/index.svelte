<script>
    let { text = '', children = null } = $props();
</script>

{#if text || children}
    <p class="error">
        {text}
        {#if children}
            {@render children()}
        {/if}
    </p>
{/if}
