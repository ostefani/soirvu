<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    let { text = '' } = $props();
</script>

{#if text}
    <p class="warning">{text}</p>
{/if}
