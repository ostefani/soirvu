<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
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
