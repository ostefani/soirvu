<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import * as api from '@/api.js';
    import { resolveErrorText } from '@/lib/errors.js';
    import { FOLDER_TREE_ERRORS } from './errors.js';
    import { breadcrumb, buildTree, descendantIds, flattenTree } from '@/lib/folders.js';

    let {
        folders = [],
        selection = { kind: 'all' },
        onSelect,
        onChanged,
    } = $props();

    const tree = $derived(buildTree(folders));
    const byId = $derived(new Map(folders.map((f) => [f.id, f])));
    const rows = $derived(flattenTree(tree.roots, collapsed));
    const recoveredRows = $derived(flattenTree(tree.recovered, collapsed));

    let collapsed = $state(new Set());
    let openMenu = $state(null); // folder id with its ⋯ menu open
    let editing = $state(null); // { mode, id?, parentId? }
    let nameValue = $state('');
    let moveTarget = $state('root');
    let treeNote = $state('');

    // Screen copy comes from the local table; unknown codes use the
    // single shared rule instead of leaking the raw IPC code.
    function describeError(e) {
        return resolveErrorText(FOLDER_TREE_ERRORS, e).text;
    }

    function isSelected(id) {
        return selection.kind === 'folder' && selection.id === id;
    }

    function selectFolder(id) {
        // Reselecting the open folder keeps its descendants toggle.
        if (isSelected(id)) return;
        onSelect?.({ kind: 'folder', id, includeDescendants: false });
    }

    function toggle(id) {
        if (collapsed.has(id)) collapsed.delete(id);
        else collapsed.add(id);
        collapsed = new Set(collapsed);
    }

    function closeForms() {
        editing = null;
        openMenu = null;
    }

    function openCreate(parentId) {
        nameValue = '';
        treeNote = '';
        editing = { mode: 'create', parentId };
        openMenu = null;
    }

    function openRename(folder) {
        nameValue = folder.name;
        treeNote = '';
        editing = { mode: 'rename', id: folder.id };
        openMenu = null;
    }

    function openMove(folder) {
        moveTarget = folder.parent_id ?? 'root';
        treeNote = '';
        editing = { mode: 'move', id: folder.id };
        openMenu = null;
    }

    function openDelete(folder) {
        treeNote = '';
        editing = { mode: 'delete', id: folder.id };
        openMenu = null;
    }

    function onKeyEscape(e) {
        if (e.key === 'Escape') closeForms();
    }

    async function createFolder() {
        const name = nameValue.trim();
        if (!name) {
            treeNote = 'Give the folder a name.';
            return;
        }
        try {
            await api.folderCreate(editing.parentId, name);
            closeForms();
            treeNote = '';
            onChanged?.();
        } catch (e) {
            treeNote = describeError(e);
        }
    }

    async function renameFolder() {
        const folder = byId.get(editing.id);
        if (!folder) {
            treeNote = describeError('folder_not_found');
            closeForms();
            onChanged?.();
            return;
        }
        const name = nameValue.trim();
        if (!name) {
            treeNote = 'Give the folder a name.';
            return;
        }
        if (name === folder.name) {
            closeForms();
            return;
        }
        try {
            await api.folderUpdate(folder.id, folder.rev, folder.parent_id, name);
            closeForms();
            treeNote = '';
            onChanged?.();
        } catch (e) {
            if (e === 'revision_conflict' || e === 'folder_not_found') onChanged?.();
            treeNote = describeError(e);
        }
    }

    async function moveFolder() {
        const folder = byId.get(editing.id);
        if (!folder) {
            treeNote = describeError('folder_not_found');
            closeForms();
            onChanged?.();
            return;
        }
        const parentId = moveTarget === 'root' ? null : moveTarget;
        if (parentId === folder.parent_id) {
            closeForms();
            return;
        }
        try {
            await api.folderUpdate(folder.id, folder.rev, parentId, folder.name);
            closeForms();
            treeNote = '';
            onChanged?.();
        } catch (e) {
            if (e === 'revision_conflict' || e === 'folder_not_found') onChanged?.();
            treeNote = describeError(e);
        }
    }

    async function deleteFolder() {
        const folder = byId.get(editing.id);
        if (!folder) {
            treeNote = describeError('folder_not_found');
            closeForms();
            onChanged?.();
            return;
        }
        try {
            await api.folderDelete(folder.id, folder.rev);
            closeForms();
            treeNote = '';
            onChanged?.();
        } catch (e) {
            if (e === 'revision_conflict' || e === 'folder_not_found') onChanged?.();
            treeNote = describeError(e);
        }
    }

    function childCount(id) {
        let n = 0;
        for (const f of folders) if (f.parent_id === id) n++;
        return n;
    }

    function crumb(id) {
        return breadcrumb(byId, id).join(' / ');
    }
</script>

{#snippet folderRow(node, depth, showRecovered)}
    {@const f = node.folder}
    {@const expanded = !collapsed.has(f.id)}
    <li>
        <div class="folder-row" class:active={isSelected(f.id)} style="--depth: {depth}">
            {#if node.children.length > 0}
                <button
                    class="disclosure"
                    aria-expanded={expanded}
                    aria-label={expanded ? `Collapse ${f.name}` : `Expand ${f.name}`}
                    onclick={() => toggle(f.id)}>
                    {expanded ? '▾' : '▸'}
                </button>
            {:else}
                <span class="disclosure-spacer" aria-hidden="true"></span>
            {/if}
            <button class="folder-name" onclick={() => selectFolder(f.id)}>
                {f.name}
                {#if showRecovered}<span class="recovered-tag">recovered</span>{/if}
            </button>
            <span class="count" title={`${f.entry_count} ${f.entry_count === 1 ? 'entry' : 'entries'}`}>
                <span class="visually-hidden">
                    {f.entry_count}
                    {f.entry_count === 1 ? 'entry' : 'entries'}
                </span>
                <span aria-hidden="true">{f.entry_count}</span>
            </span>
            <button
                class="menu-btn"
                aria-label={`Actions for ${f.name}`}
                aria-expanded={openMenu === f.id}
                onclick={() => ((openMenu = openMenu === f.id ? null : f.id), (editing = null))}>
                ⋯
            </button>
        </div>
        {#if openMenu === f.id}
            <div class="menu" onkeydown={onKeyEscape}>
                <button onclick={() => openCreate(f.id)}>New subfolder</button>
                <button onclick={() => openRename(f)}>Rename</button>
                <button onclick={() => openMove(f)}>Move</button>
                <button onclick={() => openDelete(f)}>Remove</button>
            </div>
        {/if}
        {#if editing?.mode === 'create' && editing.parentId === f.id}
            <div class="inline-form indented" onkeydown={onKeyEscape}>
                <input
                    type="text"
                    placeholder="Folder name"
                    aria-label={`New subfolder of ${f.name}`}
                    autofocus
                    bind:value={nameValue}
                    onkeydown={(e) => e.key === 'Enter' && createFolder()} />
                <div class="form-actions">
                    <button class="mini primary" onclick={createFolder}>Create</button>
                    <button class="mini" onclick={closeForms}>Cancel</button>
                </div>
            </div>
        {/if}
        {#if editing?.mode === 'rename' && editing.id === f.id}
            <div class="inline-form indented" onkeydown={onKeyEscape}>
                <input
                    type="text"
                    aria-label={`Rename ${f.name}`}
                    autofocus
                    bind:value={nameValue}
                    onkeydown={(e) => e.key === 'Enter' && renameFolder()} />
                <div class="form-actions">
                    <button class="mini primary" onclick={renameFolder}>Rename</button>
                    <button class="mini" onclick={closeForms}>Cancel</button>
                </div>
            </div>
        {/if}
        {#if editing?.mode === 'move' && editing.id === f.id}
            {@const excluded = descendantIds(folders, f.id)}
            <div class="inline-form indented" onkeydown={onKeyEscape}>
                <label class="move-label">
                    Move {f.name} to
                    <select aria-label={`Move ${f.name} to`} autofocus bind:value={moveTarget}>
                        <option value="root">Vault root</option>
                        {#each folders as dest (dest.id)}
                            {#if !excluded.has(dest.id)}
                                <option value={dest.id}>{crumb(dest.id)}</option>
                            {/if}
                        {/each}
                    </select>
                </label>
                <div class="form-actions">
                    <button class="mini primary" onclick={moveFolder}>Move</button>
                    <button class="mini" onclick={closeForms}>Cancel</button>
                </div>
            </div>
        {/if}
        {#if editing?.mode === 'delete' && editing.id === f.id}
            {@const kids = childCount(f.id)}
            <div class="inline-form indented" onkeydown={onKeyEscape}>
                {#if f.entry_count > 0 || kids > 0}
                    <p class="note">
                        “{f.name}” holds
                        {f.entry_count > 0 ? `${f.entry_count} ${f.entry_count === 1 ? 'entry' : 'entries'}` : ''}{f.entry_count > 0 && kids > 0 ? ' and ' : ''}{kids > 0 ? `${kids} ${kids === 1 ? 'subfolder' : 'subfolders'}` : ''}
                        and cannot be removed.
                    </p>
                    <div class="form-actions">
                        <button class="mini" onclick={closeForms}>Keep it</button>
                    </div>
                {:else}
                    <p class="note">Remove “{f.name}”? It holds nothing.</p>
                    <div class="form-actions">
                        <button class="mini danger" onclick={deleteFolder}>Remove</button>
                        <button class="mini" onclick={closeForms}>Keep it</button>
                    </div>
                {/if}
            </div>
        {/if}
    </li>
{/snippet}

<div class="folder-tree">
    <button
        class="scope-row"
        class:active={selection.kind === 'all'}
        onclick={() => onSelect?.({ kind: 'all' })}>
        All entries
    </button>
    <button
        class="scope-row"
        class:active={selection.kind === 'root'}
        onclick={() => onSelect?.({ kind: 'root' })}>
        Root entries
    </button>

    <div class="folders-head">
        <span class="folders-title">Folders</span>
        <button class="new-root" aria-label="New folder at the vault root" onclick={() => openCreate(null)}>
            +
        </button>
    </div>

    {#if editing?.mode === 'create' && editing.parentId == null}
        <div class="inline-form" onkeydown={onKeyEscape}>
            <input
                type="text"
                placeholder="Folder name"
                aria-label="New folder name"
                autofocus
                bind:value={nameValue}
                onkeydown={(e) => e.key === 'Enter' && createFolder()} />
            <div class="form-actions">
                <button class="mini primary" onclick={createFolder}>Create</button>
                <button class="mini" onclick={closeForms}>Cancel</button>
            </div>
        </div>
    {/if}

    {#if treeNote}<p class="note" role="status">{treeNote}</p>{/if}

    <ul class="tree">
        {#each rows as { node, depth } (node.folder.id)}
            {@render folderRow(node, depth, false)}
        {/each}
    </ul>

    {#if recoveredRows.length > 0}
        <p class="recovered-head">Recovered</p>
        <ul class="tree">
            {#each recoveredRows as { node, depth } (node.folder.id)}
                {@render folderRow(node, depth, true)}
            {/each}
        </ul>
    {/if}
</div>

<style src="./styles.css"></style>
