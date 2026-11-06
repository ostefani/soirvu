<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import {
        TOOLBAR_CODE_BLOCK,
        TOOLBAR_FORMAT_COMMANDS,
        TOOLBAR_LIST_COMMANDS,
        TOOLBAR_TABLE_COMMANDS,
        TOOLBAR_TEXT,
        TOOLBAR_TYPE_COMMANDS,
    } from '@/containers/EntryEditorContainer/data.js';

    let { selection, disabled = false, onCommand = null, onAttach = null, onLinkEntry = null } = $props();

    function run(name, arg) {
        if (arg === undefined) onCommand?.(name);
        else onCommand?.(name, arg);
    }

    function keepFocus(event) {
        event.preventDefault();
    }

    const visibleTableCommands = $derived(
        TOOLBAR_TABLE_COMMANDS.filter((command) => !command.inTableOnly || selection?.inTable),
    );
</script>

<div
    class="editor-toolbar"
    class:frozen={disabled}
    inert={disabled}
    role="toolbar"
    aria-label={TOOLBAR_TEXT.toolbarLabel}>
    {#each TOOLBAR_FORMAT_COMMANDS as command (command.id)}
        <button
            type="button"
            class={command.buttonClass}
            class:active={command.active(selection)}
            onmousedown={keepFocus}
            onclick={() => run(command.id)}
            title={command.title}><span class={command.textClass}>{command.text}</span></button>
    {/each}
    {#if onLinkEntry}
        <button
            type="button"
            class:active={!!selection?.linkActive}
            aria-pressed={!!selection?.linkActive}
            onmousedown={keepFocus}
            onclick={() => onLinkEntry()}
            title={TOOLBAR_TEXT.linkEntryTitle}>{TOOLBAR_TEXT.linkEntry}</button>
    {/if}
    {#if selection?.block === 'code_block'}
        <input
            class="tb-lang"
            type="text"
            placeholder={TOOLBAR_TEXT.languagePlaceholder}
            value={selection.codeLang || ''}
            onchange={(e) => run('setCodeLanguage', e.currentTarget.value)} />
    {/if}
    <span class="tb-sep"></span>
    {#each TOOLBAR_TYPE_COMMANDS as command (command.id)}
        <button
            type="button"
            class:active={command.active(selection)}
            onmousedown={keepFocus}
            onclick={() => run(command.id)}
            title={command.title}>{command.text}</button>
    {/each}
    <span class="tb-sep"></span>
    {#each TOOLBAR_LIST_COMMANDS as command (command.id)}
        <button
            type="button"
            class:active={command.active(selection)}
            onmousedown={keepFocus}
            onclick={() => run(command.id)}
            title={command.title}>{command.text}</button>
    {/each}
    <span class="tb-sep"></span>
    <button
        type="button"
        class:active={selection?.block === 'code_block'}
        onmousedown={keepFocus}
        onclick={() => run(TOOLBAR_CODE_BLOCK.id)}
        title={TOOLBAR_CODE_BLOCK.title}>{TOOLBAR_CODE_BLOCK.text}</button>
    {#if onAttach}
        <span class="tb-sep"></span>
        <button type="button" onmousedown={keepFocus} onclick={() => onAttach()}
            >{TOOLBAR_TEXT.attach}</button>
    {/if}
    <span class="tb-sep"></span>
    {#each visibleTableCommands as command (command.id)}
        <button
            type="button"
            onmousedown={keepFocus}
            onclick={() => run(command.id)}
            title={command.title}>{command.text}</button>
    {/each}
</div>

<style src="./styles.css"></style>
