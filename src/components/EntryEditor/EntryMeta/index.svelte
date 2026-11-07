<script>
    import { ENTRY_EDITOR_TEXT } from '../data.js';
    import { MOOD_OPTIONS } from '../constants.js';
    import { moodChipLabel, tagsChipLabel } from '../labels.js';
    import { mergeTags, parseTags } from '@/lib/tags.js';

    let { entryDate, mood, tags, frozen, pickerOpen, paneEl, panels, copying, onCopyNoteLink, onField } = $props();

    let openPop = $state(null);
    let backlinksButton = $state.raw(null);
    let backlinksStyle = $state('');

    function trackBacklinksButton(node) {
        backlinksButton = node;
        return () => { backlinksButton = null; };
    }

    $effect(() => {
        if (openPop !== 'backlinks' || !paneEl || !backlinksButton) return;
        const position = () => {
            const pane = paneEl.getBoundingClientRect();
            const button = backlinksButton.getBoundingClientRect();
            const width = Math.max(0, Math.min(360, pane.width - 16, window.innerWidth - 32));
            const left = Math.max(pane.left + 8, Math.min(button.left, pane.right - width - 8));
            backlinksStyle = `width: ${width}px; left: ${left - button.left}px; right: auto;`;
        };
        position();
        const observer = new ResizeObserver(position);
        observer.observe(paneEl);
        observer.observe(backlinksButton);
        return () => observer.disconnect();
    });

    function togglePop(name) {
        openPop = openPop === name ? null : name;
    }

    function setTags(next) {
        onField('tags', next);
    }

    function removeTag(name) {
        setTags(tags.filter((t) => t !== name));
    }

    function addTag(event) {
        if (event.key !== 'Enter' && event.key !== ',') return;
        const values = parseTags(event.currentTarget.value);
        if (values.length === 0) return;
        event.preventDefault();
        event.currentTarget.value = '';
        const next = mergeTags(tags, values);
        if (next !== tags) setTags(next);
    }

    function formatDate(d) {
        if (!d) return ENTRY_EDITOR_TEXT.dateChipFallback;
        const dt = new Date(`${d}T00:00:00`);
        if (Number.isNaN(dt.getTime())) return d;
        return dt.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
    }

    function onDateChange(e) {
        onField('entryDate', e.currentTarget.value);
    }

    function setMood(value) {
        onField('mood', value);
    }

    function handleEscape(event) {
        if (event.key !== 'Escape') return;
        if (openPop === 'backlinks') backlinksButton?.focus();
        openPop = null;
    }

    function dismissOutside(event) {
        if (!event.target.closest('.chip-wrap')) openPop = null;
    }

    $effect(() => {
        if (frozen) openPop = null;
    });
</script>

<svelte:window onkeydown={handleEscape} />
<svelte:document onclickcapture={dismissOutside} />

{#snippet chip(id, icon, label, heading, empty, content)}
    <div class="chip-wrap">
        <button
            class="meta-chip"
            class:empty
            aria-expanded={openPop === id}
            disabled={frozen}
            onclick={() => togglePop(id)}>
            <span class="chip-icon">{icon}</span>
            {label}
            <span class="chev">▾</span>
        </button>
        {#if openPop === id}
            <div class="popover">
                <h4>{heading}</h4>
                {@render content()}
            </div>
        {/if}
    </div>
{/snippet}

<div class="meta-row" inert={pickerOpen}>
    <div class="meta-chips">
        {#snippet dateField()}
            <input type="date" value={entryDate} onchange={onDateChange} />
        {/snippet}
        {#snippet moodField()}
            <div class="mood-grid">
                {#each MOOD_OPTIONS as option (option.value)}
                    <button
                        class:selected={mood === option.value}
                        onclick={() => {
                            setMood(option.value);
                            openPop = null;
                        }}>{option.label}</button>
                {/each}
            </div>
            {#if mood !== ''}
                <button class="mood-clear" onclick={() => setMood('')}
                    >{ENTRY_EDITOR_TEXT.clearMood}</button>
            {/if}
        {/snippet}
        {#snippet tagsField()}
            {#if tags.length > 0}
                <div class="tag-flow">
                    {#each tags as tag}
                        <span class="tag-pill">
                            <span class="tag-name">{tag}</span>
                            <button
                                class="tag-remove"
                                title={ENTRY_EDITOR_TEXT.removeTagLabel}
                                onclick={() => removeTag(tag)}>×</button>
                        </span>
                    {/each}
                </div>
            {/if}
            <input
                class="tag-entry"
                placeholder={ENTRY_EDITOR_TEXT.addTagPlaceholder}
                onkeydown={addTag} />
        {/snippet}

        {@render chip('date', '📅', formatDate(entryDate), ENTRY_EDITOR_TEXT.dateHeading, !entryDate, dateField)}
        {@render chip('mood', '★', moodChipLabel(mood), ENTRY_EDITOR_TEXT.moodHeading, mood === '', moodField)}
        {@render chip('tags', '#', tagsChipLabel(tags), ENTRY_EDITOR_TEXT.tagsHeading, tags.length === 0, tagsField)}
        <div class="chip-wrap">
            <button
                class="meta-chip"
                {@attach trackBacklinksButton}
                aria-expanded={openPop === 'backlinks'}
                aria-controls="entry-backlinks"
                disabled={frozen}
                onclick={() => togglePop('backlinks')}>
                {ENTRY_EDITOR_TEXT.linkedFrom}
                <span class="chev">▾</span>
            </button>
            <div
                class="popover backlinks-popover"
                style={backlinksStyle}
                id="entry-backlinks"
                hidden={openPop !== 'backlinks'}>
                <h4>{ENTRY_EDITOR_TEXT.linkedFrom}</h4>
                {@render panels?.(openPop === 'backlinks')}
            </div>
        </div>
        <button class="meta-chip" disabled={frozen || copying} onclick={() => onCopyNoteLink?.()}>
            {copying ? ENTRY_EDITOR_TEXT.copyingLink : ENTRY_EDITOR_TEXT.copyNoteLink}
        </button>
    </div>
</div>

<style src="./styles.css"></style>
