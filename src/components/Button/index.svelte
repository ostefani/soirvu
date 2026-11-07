<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
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
        title = '',
        children = null,
        ...rest
    } = $props();
</script>

<button {name} {type} {title} class={[kind, className, { active }]} {disabled} {...rest} onclick={() => handler?.()}>
    {#if children}
        {@render children()}
    {:else}
        {label}
    {/if}
</button>

<style src="./styles.css"></style>
