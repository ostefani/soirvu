<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { untrack } from 'svelte';
    import { anchoredPopupPosition } from '@/lib/popup-position.js';

    let { anchor, actions = [], label, onSelect, onClose } = $props();

    function close(restoreFocus = false) {
        const trigger = anchor;
        onClose?.();
        if (restoreFocus && trigger?.isConnected) trigger.focus({ preventScroll: true });
    }

    function mountMenu(node) {
        const trigger = untrack(() => anchor);
        const position = () => {
            if (!trigger?.isConnected) return close();
            const point = anchoredPopupPosition(trigger.getBoundingClientRect(), node.getBoundingClientRect(), {
                width: window.innerWidth,
                height: window.innerHeight,
            });
            node.style.left = `${point.left}px`;
            node.style.top = `${point.top}px`;
        };
        const outside = (event) => {
            if (!node.contains(event.target) && !trigger.contains(event.target)) close();
        };
        const scroll = (event) => {
            if (!node.contains(event.target)) close();
        };
        position();
        const observer = new ResizeObserver(position);
        observer.observe(node);
        observer.observe(trigger);
        if (trigger.parentElement) observer.observe(trigger.parentElement);
        window.addEventListener('resize', position);
        document.addEventListener('pointerdown', outside, true);
        document.addEventListener('scroll', scroll, true);
        node.querySelector('button')?.focus({ preventScroll: true });
        return () => {
            observer.disconnect();
            window.removeEventListener('resize', position);
            document.removeEventListener('pointerdown', outside, true);
            document.removeEventListener('scroll', scroll, true);
        };
    }

    function keydown(event) {
        if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            close(true);
            return;
        }
        if (event.key === 'Tab') {
            close(true);
            return;
        }
        const buttons = [...event.currentTarget.querySelectorAll('button')];
        const current = buttons.indexOf(document.activeElement);
        const index = {
            ArrowDown: (current + 1) % buttons.length,
            ArrowUp: (current - 1 + buttons.length) % buttons.length,
            Home: 0,
            End: buttons.length - 1,
        }[event.key];
        if (index === undefined) return;
        event.preventDefault();
        buttons[index]?.focus({ preventScroll: true });
    }
</script>

<div class="menu" role="menu" tabindex="-1" aria-label={label} onkeydown={keydown} {@attach mountMenu}>
    {#each actions as action (action.id)}
        <button type="button" role="menuitem" onclick={() => onSelect?.(action.id)}>{action.label}</button>
    {/each}
</div>

<style src="./styles.css"></style>
