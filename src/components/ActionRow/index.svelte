<script>
    import Button from '@/components/Button/index.svelte';
    import { useActions } from './context.js';

    /**
     * A row of buttons built from action names.
     *
     * Each name is looked up in ACTIONS (label, icon, …), handlers and disabled,
     * which the screen provides once through `provideActions`.
     *
     * - `kind`      button kind for every action; `null` leaves the kind to ACTIONS.
     * - `kinds`     per-action override, e.g. `{ 'change-passphrase': 'primary-button' }`.
     * - `separated` draws a rule above the row (secondary / destructive group).
     * - `children`  extra buttons rendered after the named ones.
     *
     * @type {{
     *   names?: string[],
     *   kind?: string | null,
     *   kinds?: Record<string, string>,
     *   separated?: boolean,
     *   children?: import('svelte').Snippet,
     * }}
     */
    let { names = [], kind = 'texture-button', kinds = {}, separated = false, children } = $props();

    const source = useActions();

    /** @param {string} name */
    function kindProps(name) {
        const resolved = kinds[name] ?? kind;
        return resolved ? { kind: resolved } : {};
    }
</script>

{#if names?.length || children}
    <div class="action-row" class:separated>
        {#each names ?? [] as name (name)}
            <Button
                {...source.ACTIONS[name]}
                {...kindProps(name)}
                handler={source.handlers[name]}
                disabled={source.disabled[name]} />
        {/each}
        {@render children?.()}
    </div>
{/if}

<style src="./styles.css"></style>
