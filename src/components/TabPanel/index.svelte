<script>
    /**
     * Content area for one tab. `id` must match the tab's id: the panel gets
     * `panel-<id>` and is labelled by `tab-<id>`, which is what Tabs renders.
     *
     * @type {{ id: string, children?: import('svelte').Snippet }}
     */
    let { id, children } = $props();
</script>

<div class="tab-panel" role="tabpanel" id="panel-{id}" aria-labelledby="tab-{id}">
    {@render children?.()}
</div>

<style src="./styles.css"></style>
