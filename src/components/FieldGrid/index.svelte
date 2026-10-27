<script>
    import Input from '@/components/Input/index.svelte';

    /**
     * Two-column form built from field definitions.
     *
     * - `fields`        field objects as Input expects them.
     * - `getValue`      (field) => current value.
     * - `onInput`       (field, value) => void.
     * - `secondColumn`  indexes of fields that start in the right column,
     *                   leaving the left cell of that row empty.
     * - `children`      full-width footer (action rows, progress, notes).
     *
     * @type {{
     *   fields?: any[],
     *   getValue: (field: any) => string,
     *   onInput: (field: any, value: string) => void,
     *   secondColumn?: number[],
     *   children?: import('svelte').Snippet,
     * }}
     */
    let { fields = [], getValue, onInput, secondColumn = [], children } = $props();
</script>

<form class="field-grid">
    {#each fields ?? [] as field, i (field.name ?? i)}
        <div class="field" class:second-column={secondColumn.includes(i)}>
            <Input {field} value={getValue(field)} oninput={(e) => onInput(field, e.currentTarget.value)} />
        </div>
    {/each}

    {#if children}
        <div class="field-grid-footer">
            {@render children()}
        </div>
    {/if}
</form>

<style src="./styles.css"></style>
