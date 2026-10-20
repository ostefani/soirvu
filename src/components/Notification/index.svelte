<script>
    let {
        event = null, // 'created' | 'deleted' | 'updated' | 'saved' | 'copied' | 'error' | null
        type = null, // 'success' | 'info' | 'warning' | 'danger' | 'neutral' | null
        title = '',
        message = '',
        text = '',
        duration = 0, // Auto-dismiss time in ms (0 = persistent)
        dismissible = true,
        mode = 'inline', // 'inline' | 'toast'
        onDismiss = null,
        children = null,
    } = $props();

    let dismissed = $state(false);

    // Compute effective semantic type based on event preset or explicit type
    const computedType = $derived.by(() => {
        if (type) return type;
        switch (event) {
            case 'created':
            case 'saved':
            case 'updated':
                return 'success';
            case 'deleted':
                return 'danger';
            case 'copied':
                return 'info';
            case 'error':
                return 'danger';
            default:
                return 'info';
        }
    });

    // Compute default title from event if none provided
    const computedTitle = $derived.by(() => {
        if (title) return title;
        switch (event) {
            case 'created':
                return 'Created';
            case 'saved':
                return 'Saved';
            case 'updated':
                return 'Updated';
            case 'deleted':
                return 'Deleted';
            case 'copied':
                return 'Copied';
            case 'error':
                return 'Error';
            default:
                return '';
        }
    });

    const displayMessage = $derived(message || text);

    function handleClose() {
        dismissed = true;
        onDismiss?.();
    }

    $effect(() => {
        if (duration > 0 && !dismissed) {
            const timer = setTimeout(() => {
                handleClose();
            }, duration);
            return () => clearTimeout(timer);
        }
    });
</script>

{#if !dismissed && (displayMessage || children || computedTitle)}
    <div
        class="notification-banner"
        class:mode-toast={mode === 'toast'}
        class:type-success={computedType === 'success'}
        class:type-info={computedType === 'info'}
        class:type-warning={computedType === 'warning'}
        class:type-danger={computedType === 'danger'}
        class:type-neutral={computedType === 'neutral'}
        role="alert">
        <span class="notification-icon" aria-hidden="true">
            {#if computedType === 'success'}
                <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                    <path d="M16 8A8 8 0 1 1 0 8a8 8 0 0 1 16 0zm-3.97-3.03a.75.75 0 0 0-1.08.022L7.477 9.417 5.384 7.323a.75.75 0 0 0-1.06 1.06L6.97 11.03a.75.75 0 0 0 1.079-.02l3.992-4.99a.75.75 0 0 0-.01-1.05z"/>
                </svg>
            {:else if computedType === 'danger'}
                <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                    <path d="M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14zm0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16z"/>
                    <path d="M7.002 11a1 1 0 1 1 2 0 1 1 0 0 1-2 0zM7.1 4.995a.905.905 0 1 1 1.8 0l-.35 3.507a.552.552 0 0 1-1.1 0L7.1 4.995z"/>
                </svg>
            {:else if computedType === 'warning'}
                <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                    <path d="M8.982 1.566a1.13 1.13 0 0 0-1.964 0L.165 13.233c-.457.778.091 1.767.982 1.767h13.706c.89 0 1.438-.99.982-1.767L8.982 1.566zM8 5c.535 0 .954.462.9.995l-.35 3.507a.552.552 0 0 1-1.1 0L7.1 5.995A.905.905 0 0 1 8 5zm.002 6a1 1 0 1 1 0 2 1 1 0 0 1 0-2z"/>
                </svg>
            {:else}
                <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                    <path d="M8 16A8 8 0 1 0 8 0a8 8 0 0 0 0 16zm.93-9.412-1 4.705c-.07.34.029.533.304.533.194 0 .487-.07.686-.246l-.088.416c-.287.346-.92.598-1.465.598-.704 0-1.002-.422-.808-1.319l.738-3.468c.064-.293.006-.399-.287-.47l-.451-.081.082-.381 2.29-.287zM8 5.5a1 1 0 1 1 0-2 1 1 0 0 1 0 2z"/>
                </svg>
            {/if}
        </span>

        <div class="notification-body">
            {#if computedTitle || event}
                <div class="notification-header">
                    {#if computedTitle}
                        <strong class="notification-title">{computedTitle}</strong>
                    {/if}
                    {#if event}
                        <span class="notification-badge">{event}</span>
                    {/if}
                </div>
            {/if}

            {#if children}
                <div class="notification-message">
                    {@render children()}
                </div>
            {:else if displayMessage}
                <p class="notification-message">{displayMessage}</p>
            {/if}
        </div>

        {#if dismissible}
            <button
                type="button"
                class="notification-close-button"
                aria-label="Dismiss notification"
                onclick={handleClose}>
                ×
            </button>
        {/if}
    </div>
{/if}

<style src="./styles.css"></style>
