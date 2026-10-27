<script>
    import CardHeader from '@/components/CardHeader/index.svelte';

    /**
     * A titled surface that groups related settings.
     *
     * The header is driven by props; the body is whatever is passed as children.
     * This component also owns the look of form controls inside a card (labels
     * above controls, selects), so every tab gets the same rhythm for free.
     *
     * @type {{
     *   title?: string,
     *   notes?: Array<string | null | undefined>,
     *   hints?: Array<string | null | undefined>,
     *   class?: string,
     *   children?: import('svelte').Snippet,
     * }}
     */
    let { title, notes, hints, class: className = '', children } = $props();
</script>

<div class="settings-card {className}">
    <CardHeader {title} {notes} {hints} />
    {@render children?.()}
</div>

<style src="./styles.css"></style>
