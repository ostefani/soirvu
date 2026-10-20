<script>
    let { text = '' } = $props();
</script>

{#if text}
    <p class="note">{text}</p>
{/if}
