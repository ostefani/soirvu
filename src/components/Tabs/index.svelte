<script>
    let {
        tabs = [],
        activeTab = '',
        onChange = null,
    } = $props();

    function handleTabClick(tabId) {
        if (tabId !== activeTab) {
            onChange?.(tabId);
        }
    }
</script>

<div class="tabs-container" role="tablist" aria-label="Sections">
    {#each tabs as tab (tab.id)}
        <button
            type="button"
            role="tab"
            class="tab-item"
            class:active={activeTab === tab.id}
            aria-selected={activeTab === tab.id}
            id={`tab-${tab.id}`}
            aria-controls={`panel-${tab.id}`}
            onclick={() => handleTabClick(tab.id)}>
            <span class="tab-label">{tab.label}</span>
            {#if tab.badge}
                <span class="tab-badge">{tab.badge}</span>
            {/if}
        </button>
    {/each}
</div>

<style src="./styles.css"></style>
