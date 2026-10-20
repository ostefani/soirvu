<script>
    let { field, value = $bindable(''), disabled = false, onenter = null, oninput = null } = $props();

    function keydown(e) {
        if (e.key === 'Enter') onenter?.();
    }
</script>

{#snippet control()}
    <input
        name={field.name}
        type={field.type ?? 'text'}
        min={field.min}
        placeholder={field.placeholder ?? ''}
        autocomplete={field.autocomplete}
        {disabled}
        bind:value
        onkeydown={keydown}
        oninput={(e) => oninput?.(e)} />
{/snippet}

{#if field.label}
    <label>
        {field.label}
        {@render control()}
    </label>
{:else}
    {@render control()}
{/if}
