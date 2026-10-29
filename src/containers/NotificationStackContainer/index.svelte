<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { slide } from 'svelte/transition';
    import { cubicOut } from 'svelte/easing';
    import { prefersReducedMotion } from 'svelte/motion';
    import Notification from '@/components/Notification/index.svelte';
    import { notificationItems, dismiss } from '@/components/Notification/store.svelte.js';

    const ms = (n) => (prefersReducedMotion.current ? 0 : n);

    function close(item) {
        dismiss(item.id);
        item.onDismiss?.();
    }
</script>

<div class="notification-stack">
    {#each notificationItems as item (item.id)}
        <div class="notification-stack-item" transition:slide={{ duration: ms(260), easing: cubicOut }}>
            <Notification
                event={item.event}
                type={item.type}
                title={item.title}
                message={item.message}
                dismissible={item.dismissible}
                onDismiss={() => close(item)} />
        </div>
    {/each}
</div>

<style src="./styles.css"></style>
