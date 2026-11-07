<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import Button from '@/components/Button/index.svelte';
    let {
        title,
        body,
        warning = '',
        confirmLabel,
        cancelLabel,
        pendingLabel,
        error = '',
        busy = false,
        tone = 'danger',
        returnFocus = null,
        onConfirm,
        onCancel,
    } = $props();
    const id = $props.id();

    function mountDialog(node) {
        node.showModal();
        node.querySelector('[data-cancel]')?.focus({ preventScroll: true });
        return () => {
            node.close();
            if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
        };
    }

    function cancel(event) {
        event.preventDefault();
        if (!busy) onCancel?.();
    }

    function backdrop(event) {
        if (event.target !== event.currentTarget || busy) return;
        const rect = event.currentTarget.getBoundingClientRect();
        if (
            event.clientX < rect.left ||
            event.clientX > rect.right ||
            event.clientY < rect.top ||
            event.clientY > rect.bottom
        )
            onCancel?.();
    }
    console.log('tone: ', tone);
</script>

<dialog
    class="confirmation-modal"
    role="alertdialog"
    aria-labelledby={`${id}-title`}
    aria-describedby={`${id}-description`}
    aria-busy={busy}
    oncancel={cancel}
    onclick={backdrop}
    {@attach mountDialog}>
    <div class="modal-content">
        <h2 id={`${id}-title`}>{title}</h2>
        <div id={`${id}-description`}>
            <p>{body}</p>
            {#if warning}<p class="warning">{warning}</p>{/if}
        </div>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div class="modal-actions">
            <Button kind={tone === 'danger' ? 'danger-button' : 'default-button'} disabled={busy} handler={onConfirm}
                >{busy ? pendingLabel : confirmLabel}</Button>
            <Button type="button" kind="quiet-button" data-cancel disabled={busy} handler={() => onCancel?.()}
                >{cancelLabel}</Button>
        </div>
    </div>
</dialog>

<style src="./styles.css"></style>
