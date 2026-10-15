<script>
    // The floating selection toolbar (§6.5): shown by EntryEditor
    // while text is selected, positioned from view.coordsAtPos.
    // This component only renders and forwards clicks — the
    // commands and their active-state logic live in
    // editor/toolbar.js. Buttons take the mousedown without
    // letting it move focus, so the editor keeps its selection.
    let { editor, bar } = $props();

    function run(name) {
        editor?.exec(name);
    }

    function keepFocus(event) {
        event.preventDefault();
    }
</script>

<div
    class="sel-toolbar"
    class:below={bar.below}
    style="left: {bar.x}px; top: {bar.y}px"
    role="toolbar"
    aria-label="Formatting"
>
    <button
        type="button"
        class:active={bar.info.strong}
        onmousedown={keepFocus}
        onclick={() => run('strong')}
        title="Bold (Ctrl+B)"><strong>B</strong></button
    >
    <button
        type="button"
        class:active={bar.info.em}
        onmousedown={keepFocus}
        onclick={() => run('em')}
        title="Italic (Ctrl+I)"><em>I</em></button
    >
    <button
        type="button"
        class="tb-strike"
        class:active={bar.info.strike}
        onmousedown={keepFocus}
        onclick={() => run('strike')}
        title="Strikethrough (Ctrl+Shift+X)">S</button
    >
    <button
        type="button"
        class:active={bar.info.code}
        onmousedown={keepFocus}
        onclick={() => run('code')}
        title="Inline code (Ctrl+E)">&lt;/&gt;</button
    >
    <span class="tb-sep"></span>
    <button
        type="button"
        class:active={bar.info.block === 'paragraph' && !bar.info.list}
        onmousedown={keepFocus}
        onclick={() => run('paragraph')}
        title="Paragraph">¶</button
    >
    <button
        type="button"
        class:active={bar.info.headingLevel === 1}
        onmousedown={keepFocus}
        onclick={() => run('heading1')}
        title="Heading 1">H1</button
    >
    <button
        type="button"
        class:active={bar.info.headingLevel === 2}
        onmousedown={keepFocus}
        onclick={() => run('heading2')}
        title="Heading 2">H2</button
    >
    <button
        type="button"
        class:active={bar.info.headingLevel === 3}
        onmousedown={keepFocus}
        onclick={() => run('heading3')}
        title="Heading 3">H3</button
    >
    <span class="tb-sep"></span>
    <button
        type="button"
        class:active={bar.info.quote}
        onmousedown={keepFocus}
        onclick={() => run('quote')}
        title="Quote">❝</button
    >
    <button
        type="button"
        class:active={bar.info.list === 'bullet'}
        onmousedown={keepFocus}
        onclick={() => run('bulletList')}
        title="Bullet list">• List</button
    >
    <button
        type="button"
        class:active={bar.info.list === 'ordered'}
        onmousedown={keepFocus}
        onclick={() => run('orderedList')}
        title="Numbered list">1. List</button
    >
    <button
        type="button"
        class:active={bar.info.list === 'task'}
        onmousedown={keepFocus}
        onclick={() => run('taskList')}
        title="Task list">☑ Task</button
    >
    <span class="tb-sep"></span>
    <button
        type="button"
        class:active={bar.info.block === 'code_block'}
        onmousedown={keepFocus}
        onclick={() => run('codeBlock')}
        title="Code block">{'{ }'}</button
    >
</div>

<style>
    .sel-toolbar {
        position: fixed;
        z-index: 40;
        display: flex;
        align-items: center;
        gap: 2px;
        padding: 4px;
        background: #ffffff;
        border: 1px solid #e2e2e2;
        border-radius: 10px;
        box-shadow: 0 6px 24px rgba(0, 0, 0, 0.16);
        transform: translate(-50%, calc(-100% - 8px));
    }

    .sel-toolbar.below {
        transform: translate(-50%, 8px);
    }

    .sel-toolbar button {
        min-width: 28px;
        height: 28px;
        padding: 0 6px;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: #333;
        font-size: 13px;
    }

    .sel-toolbar button:hover {
        background: #f0f0f0;
    }

    .sel-toolbar button.active {
        background: #e3ecfd;
        color: #2563eb;
    }

    .tb-strike {
        text-decoration: line-through;
    }

    .tb-sep {
        width: 1px;
        align-self: stretch;
        margin: 3px 3px;
        background: #e2e2e2;
    }
</style>
