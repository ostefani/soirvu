<script>
    import { EVENT_TYPE } from '@/constants.js';

    let {
        event = null,
        type = null,
        title = '',
        message = '',
        text = '',
        dismissible = true,
        mode = 'toast',
        onDismiss = null,
        children = null,
    } = $props();

    const EVENT_PRESET = {
        [EVENT_TYPE.CREATED]: { type: EVENT_TYPE.SUCCESS, title: 'Created' },
        [EVENT_TYPE.SAVED]: { type: EVENT_TYPE.SUCCESS, title: 'Saved' },
        [EVENT_TYPE.UPDATED]: { type: EVENT_TYPE.SUCCESS, title: 'Updated' },
        [EVENT_TYPE.DELETED]: { type: EVENT_TYPE.DANGER, title: 'Deleted' },
        [EVENT_TYPE.COPIED]: { type: EVENT_TYPE.INFO, title: 'Copied' },
        [EVENT_TYPE.ERROR]: { type: EVENT_TYPE.DANGER, title: 'Error' },
        [EVENT_TYPE.WARNING]: { type: EVENT_TYPE.WARNING, title: '' },
        [EVENT_TYPE.NEUTRAL]: { type: EVENT_TYPE.NEUTRAL, title: '' },
    };

    const preset = $derived(EVENT_PRESET[event] ?? { type: EVENT_TYPE.INFO, title: '' });
    const shownType = $derived(type || preset.type);
    const shownTitle = $derived(title || preset.title);
    const displayMessage = $derived(message || text);

    function handleClose() {
        onDismiss?.();
    }
</script>

{#if displayMessage || children || shownTitle}
    <div
        class="notification-banner"
        class:mode-toast={mode === 'toast'}
        class:type-success={shownType === EVENT_TYPE.SUCCESS}
        class:type-info={shownType === EVENT_TYPE.INFO}
        class:type-warning={shownType === EVENT_TYPE.WARNING}
        class:type-danger={shownType === EVENT_TYPE.DANGER}
        class:type-neutral={shownType === EVENT_TYPE.NEUTRAL}
        role="alert">
        <span class="notification-icon" aria-hidden="true">
            {#if shownType === EVENT_TYPE.SUCCESS}
                <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                    <path
                        d="M16 8A8 8 0 1 1 0 8a8 8 0 0 1 16 0zm-3.97-3.03a.75.75 0 0 0-1.08.022L7.477 9.417 5.384 7.323a.75.75 0 0 0-1.06 1.06L6.97 11.03a.75.75 0 0 0 1.079-.02l3.992-4.99a.75.75 0 0 0-.01-1.05z" />
                </svg>
            {:else if shownType === EVENT_TYPE.DANGER}
                <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                    <path d="M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14zm0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16z" />
                    <path
                        d="M7.002 11a1 1 0 1 1 2 0 1 1 0 0 1-2 0zM7.1 4.995a.905.905 0 1 1 1.8 0l-.35 3.507a.552.552 0 0 1-1.1 0L7.1 4.995z" />
                </svg>
            {:else if shownType === EVENT_TYPE.WARNING}
                <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                    <path
                        d="M8.982 1.566a1.13 1.13 0 0 0-1.964 0L.165 13.233c-.457.778.091 1.767.982 1.767h13.706c.89 0 1.438-.99.982-1.767L8.982 1.566zM8 5c.535 0 .954.462.9.995l-.35 3.507a.552.552 0 0 1-1.1 0L7.1 5.995A.905.905 0 0 1 8 5zm.002 6a1 1 0 1 1 0 2 1 1 0 0 1 0-2z" />
                </svg>
            {:else}
                <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
                    <path
                        d="M8 16A8 8 0 1 0 8 0a8 8 0 0 0 0 16zm.93-9.412-1 4.705c-.07.34.029.533.304.533.194 0 .487-.07.686-.246l-.088.416c-.287.346-.92.598-1.465.598-.704 0-1.002-.422-.808-1.319l.738-3.468c.064-.293.006-.399-.287-.47l-.451-.081.082-.381 2.29-.287zM8 5.5a1 1 0 1 1 0-2 1 1 0 0 1 0 2z" />
                </svg>
            {/if}
        </span>

        <div class="notification-body">
            {#if shownTitle || event}
                <div class="notification-header">
                    {#if shownTitle}
                        <strong class="notification-title">{shownTitle}</strong>
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
