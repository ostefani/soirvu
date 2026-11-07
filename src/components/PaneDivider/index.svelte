<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    let { label, controls, value, min, max, onResize } = $props();

    let drag = $state(null);

    function start(event) {
        if (event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        drag = { x: event.clientX, value };
    }

    function move(event) {
        if (!drag) return;
        onResize(Math.max(min, Math.min(max, drag.value + event.clientX - drag.x)));
    }

    function keydown(event) {
        const step = event.shiftKey ? 40 : 10;
        const next = {
            ArrowLeft: value - step,
            ArrowRight: value + step,
            Home: min,
            End: max,
        }[event.key];
        if (next === undefined) return;
        event.preventDefault();
        onResize(Math.max(min, Math.min(max, next)));
    }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Focusable separators implement the WAI-ARIA window splitter pattern.) -->
<div
    class="pane-divider"
    class:dragging={drag !== null}
    role="separator"
    tabindex="0"
    aria-label={label}
    aria-controls={controls}
    aria-orientation="vertical"
    aria-valuemin={min}
    aria-valuemax={max}
    aria-valuenow={Math.round(value)}
    aria-valuetext={`${Math.round(value)} pixels`}
    onpointerdown={start}
    onpointermove={move}
    onpointerup={() => (drag = null)}
    onpointercancel={() => (drag = null)}
    onlostpointercapture={() => (drag = null)}
    onkeydown={keydown}>
</div>

<style src="./styles.css"></style>
