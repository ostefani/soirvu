<script>
    import { tick } from 'svelte';
    import Button from '@/components/Button/index.svelte';
    import IconLeft from '@/icons/arrow-left.svelte';
    import { ENTRY_EDITOR_TEXT } from '../data.js';

    let { status, saveConflict, history, navigating, goBack, remove } = $props();

    let deleteArmed = $state(false);
    let deleteActions;
    let deleteWidthAnimation;

    async function toggleDeleteArmed() {
        const actions = deleteActions;
        const startWidth = actions.getBoundingClientRect().width;
        deleteWidthAnimation?.cancel();
        deleteArmed = !deleteArmed;
        if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;

        await tick();
        if (!actions.isConnected) return;
        const endWidth = actions.getBoundingClientRect().width;
        deleteWidthAnimation = actions.animate(
            { width: [`${startWidth}px`, `${endWidth}px`] },
            { duration: 120, easing: 'ease-in-out' },
        );
    }
</script>

<div class="editor-footer">
    <div class="footer-navigation">
        <Button kind="quiet-button" handler={goBack} aria-label={ENTRY_EDITOR_TEXT.home}
            ><IconLeft />{ENTRY_EDITOR_TEXT.home}</Button>
        <div class="navigation-wrapper">
            <Button
                kind="quiet-button"
                handler={history.back}
                aria-label={ENTRY_EDITOR_TEXT.goBackLabel}
                title={ENTRY_EDITOR_TEXT.goBackTitle}
                disabled={!history.canGoBack || navigating}>←</Button>
            <Button
                kind="quiet-button"
                handler={history.forward}
                aria-label={ENTRY_EDITOR_TEXT.goForwardLabel}
                title={ENTRY_EDITOR_TEXT.goForwardTitle}
                disabled={!history.canGoForward || navigating}>→</Button>
        </div>
    </div>
    <span class="status">{saveConflict ? ENTRY_EDITOR_TEXT.conflictStatus : status}</span>
    <div class="footer-actions" class:armed={deleteArmed} bind:this={deleteActions}>
        <Button
            kind="quiet-button"
            aria-expanded={deleteArmed}
            aria-label={deleteArmed ? ENTRY_EDITOR_TEXT.keepEntry : ENTRY_EDITOR_TEXT.deleteEntry}
            handler={toggleDeleteArmed}>
            <span class="delete-label" class:current={!deleteArmed} aria-hidden="true">{ENTRY_EDITOR_TEXT.deleteEntry}</span>
            <span class="delete-label" class:current={deleteArmed} aria-hidden="true">{ENTRY_EDITOR_TEXT.keepEntry}</span>
        </Button>
        <div class="footer-delete-confirmation" inert={!deleteArmed}>
            <Button kind="danger-button" class="confirm-button" handler={remove}
                >{ENTRY_EDITOR_TEXT.confirmDelete}</Button>
        </div>
    </div>
</div>

<style src="./styles.css"></style>
