<script>
    let {
        name = '',
        label = '',
        kind = '',
        class: className = '',
        active = false,
        disabled = false,
        type = 'button',
        handler = null,
        children = null,
    } = $props();
</script>

<!-- <button {name} {type} class="{kind} {className}" class:active {disabled} onclick={() => handler?.()}> -->
<button {name} {type} class={[kind, className, { active }]} {disabled} onclick={() => handler?.()}>
    {#if children}
        {@render children()}
    {:else}
        {label}
    {/if}
</button>

<style src="./styles.css"></style>
