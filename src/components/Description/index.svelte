<script>
    let {
        text = '',
        size = 'default', // 'default' | 'small' | 'large'
        subtle = false,
        children = null,
    } = $props();
</script>

<p
    class="description"
    class:size-small={size === 'small'}
    class:size-large={size === 'large'}
    class:color-subtle={subtle}>
    {#if children}
        {@render children()}
    {:else}
        {text}
    {/if}
</p>

<style src="./styles.css"></style>
