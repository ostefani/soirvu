<script>
    import { nextNotificationId, upsertNotification } from './store.svelte.js';

    let {
        id,
        event = null,
        type = null,
        title = '',
        message = '',
        text = '',
        duration = 0,
        dismissible = true,
        onDismiss = null,
    } = $props();

    let active = false;
    let activeNotificationId = '';
    let previousContent = '';

    $effect(() => {
        const content = message || text;
        if (!content && !title) {
            active = false;
            previousContent = '';
            return;
        }
        if (!id) return;

        if (!active || content !== previousContent) {
            active = true;
            previousContent = content;
            activeNotificationId = nextNotificationId(id);
        }

        upsertNotification({
            id: activeNotificationId,
            event,
            type,
            title,
            message: content,
            duration,
            dismissible,
            onDismiss,
        });
    });
</script>
