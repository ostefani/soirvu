<script>
    let {
        name = '',
        label = '',
        kind = '',
        active = false,
        disabled = false,
        type = 'button',
        handler = null,
        children = null,
    } = $props();
</script>

<button {name} {type} class={kind} class:active {disabled} onclick={() => handler?.()}>
    {#if children}
        {@render children()}
    {:else}
        {label}
    {/if}
</button>

<style src="./styles.css"></style>
