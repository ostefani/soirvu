<script>
    let { field, value = '', disabled = false, class: className = '', onenter = null, oninput = null, onupdate = null } = $props();

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
        class={className}
        {value}
        onkeydown={keydown}
        oninput={(e) => oninput?.(e)}
        onchange={(e) => onupdate?.(e)} />
{/snippet}

{#if field.label}
    <label>
        {field.label}
        {@render control()}
    </label>
{:else}
    {@render control()}
{/if}
