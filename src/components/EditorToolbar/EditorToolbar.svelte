<script>
    let { editor, info, onAttach = null } = $props();

    function run(name) {
        editor?.exec(name);
    }

    function keepFocus(event) {
        event.preventDefault();
    }
</script>
<style src="./EditorToolbar.css"></style>

<div class="editor-toolbar" role="toolbar" aria-label="Formatting">
    <button
        type="button"
        class:active={info?.strong}
        onmousedown={keepFocus}
        onclick={() => run('strong')}
        title="Bold (Ctrl+B)"><strong>B</strong></button>
    <button
        type="button"
        class:active={info?.em}
        onmousedown={keepFocus}
        onclick={() => run('em')}
        title="Italic (Ctrl+I)"><em>I</em></button>
    <button
        type="button"
        class="tb-strike"
        class:active={info?.strike}
        onmousedown={keepFocus}
        onclick={() => run('strike')}
        title="Strikethrough (Ctrl+Shift+X)">S</button>
    <button
        type="button"
        class:active={info?.code}
        onmousedown={keepFocus}
        onclick={() => run('code')}
        title="Inline code (Ctrl+E)">&lt;/&gt;</button>
    {#if info?.block === 'code_block'}
        <input
            class="tb-lang"
            type="text"
            placeholder="language"
            value={info.codeLang || ''}
            onchange={(e) => editor?.exec('setCodeLanguage', e.currentTarget.value)} />
    {/if}
    <span class="tb-sep"></span>
    <button
        type="button"
        class:active={info?.block === 'paragraph' && !info?.list}
        onmousedown={keepFocus}
        onclick={() => run('paragraph')}
        title="Paragraph">¶</button>
    <button
        type="button"
        class:active={info?.headingLevel === 1}
        onmousedown={keepFocus}
        onclick={() => run('heading1')}
        title="Heading 1">H1</button>
    <button
        type="button"
        class:active={info?.headingLevel === 2}
        onmousedown={keepFocus}
        onclick={() => run('heading2')}
        title="Heading 2">H2</button>
    <button
        type="button"
        class:active={info?.headingLevel === 3}
        onmousedown={keepFocus}
        onclick={() => run('heading3')}
        title="Heading 3">H3</button>
    <span class="tb-sep"></span>
    <button type="button" class:active={info?.quote} onmousedown={keepFocus} onclick={() => run('quote')} title="Quote"
        >❝</button>
    <button
        type="button"
        class:active={info?.list === 'bullet'}
        onmousedown={keepFocus}
        onclick={() => run('bulletList')}
        title="Bullet list">• List</button>
    <button
        type="button"
        class:active={info?.list === 'ordered'}
        onmousedown={keepFocus}
        onclick={() => run('orderedList')}
        title="Numbered list">1. List</button>
    <button
        type="button"
        class:active={info?.list === 'task'}
        onmousedown={keepFocus}
        onclick={() => run('taskList')}
        title="Task list">☑ Task</button>
    <span class="tb-sep"></span>
    <button
        type="button"
        class:active={info?.block === 'code_block'}
        onmousedown={keepFocus}
        onclick={() => run('codeBlock')}
        title="Code block">{'{ }'}</button>
    {#if onAttach}
        <span class="tb-sep"></span>
        <button type="button" onmousedown={keepFocus} onclick={() => onAttach()}>Attach…</button>
    {/if}
    <span class="tb-sep"></span>
    <button type="button" onmousedown={keepFocus} onclick={() => run('insertTable')} title="Insert table">Table</button>
    {#if info?.inTable}
        <button type="button" onmousedown={keepFocus} onclick={() => run('addRow')} title="Add a row below"
            >+Row</button>
        <button type="button" onmousedown={keepFocus} onclick={() => run('addColumn')} title="Add a column to the right"
            >+Col</button>
        <button type="button" onmousedown={keepFocus} onclick={() => run('deleteRow')} title="Delete this row"
            >−Row</button>
        <button type="button" onmousedown={keepFocus} onclick={() => run('deleteColumn')} title="Delete this column"
            >−Col</button>
    {/if}
</div>
