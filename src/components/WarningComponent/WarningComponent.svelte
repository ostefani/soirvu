<script>
    let { text = '' } = $props();
</script>

{#if text}
    <p class="warning">{text}</p>
{/if}
