<script>
    import { flip } from 'svelte/animate';
    import { fade, fly } from 'svelte/transition';
    import NotificationView from './NotificationView.svelte';
    import { notificationItems, removeNotification } from './store.svelte.js';

    function dismiss(item) {
        removeNotification(item.id);
        item.onDismiss?.(item.message);
    }
</script>

<div class="notification-stack" aria-live="polite" aria-relevant="additions removals">
    {#each notificationItems as item (item.id)}
        <div class="notification-stack-item" animate:flip={{ duration: 220 }} in:fly={{ y: -12, duration: 220 }} out:fade={{ duration: 160 }}>
            <NotificationView
                event={item.event}
                type={item.type}
                title={item.title}
                message={item.message}
                duration={item.duration}
                dismissible={item.dismissible}
                onDismiss={() => dismiss(item)} />
        </div>
    {/each}
</div>

<style>
    .notification-stack {
        position: fixed;
        top: max(1rem, env(safe-area-inset-top));
        right: max(1rem, env(safe-area-inset-right));
        z-index: 1000;
        display: flex;
        flex-direction: column;
        align-items: flex-end;
        gap: 0.65rem;
        width: min(28rem, calc(100vw - 2rem));
        max-height: calc(100vh - 2rem);
        overflow-y: auto;
        overflow-x: hidden;
        pointer-events: none;
    }

    .notification-stack-item {
        width: 100%;
        pointer-events: auto;
    }

    @media (prefers-reduced-motion: reduce) {
        .notification-stack-item { transition: none !important; }
    }
</style>
