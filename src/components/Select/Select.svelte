<script>
    let { field, value = $bindable(''), disabled = false, onchange = null } = $props();
</script>

{#snippet control()}
    <select name={field.name} {disabled} bind:value onchange={() => onchange?.()}>
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
