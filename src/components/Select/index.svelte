<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    // Controlled value: callers pass the current option and read the
    // next one from onchange. No two-way binding is advertised.
    let { field, value = '', disabled = false, onchange = null } = $props();
</script>

{#snippet control()}
    <select name={field.name} {disabled} value={value} onchange={(e) => onchange?.(e)}>
        {#each field.options as option (option.value)}
            <option value={option.value}>{option.label}</option>
        {/each}
    </select>
{/snippet}

{#if field.label}
    <label>
        {field.label}
        {@render control()}
    </label>
{:else}
    {@render control()}
{/if}
