<script>
    /**
     * Spinner with a status line, for operations that are running.
     * @type {{ text: string }}
     */
    let { text } = $props();
</script>

<div class="progress" role="status">
    <span class="spinner" aria-hidden="true"></span>
    <span>{text}</span>
</div>

<style src="./styles.css"></style>
