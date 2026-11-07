<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import AnchoredMenu from '@/components/AnchoredMenu/index.svelte';
    import ConfirmationModalContainer from '@/containers/ConfirmationModalContainer/index.svelte';
    import { FOLDER_ACTIONS, FOLDER_DELETE_MODAL, folderDeleteBody } from './data.js';
    import { untrack } from 'svelte';
    import { folderPathIds } from '@/lib/entry-location.js';
    import * as api from '@/api.js';
    import { resolveErrorText } from '@/lib/errors.js';
    import { FOLDER_TREE_ERRORS } from './errors.js';
    import { breadcrumb, buildTree, descendantIds, flattenTree } from '@/lib/folders.js';

    let {
        folders = [],
        selection = { kind: 'root' },
        revealVersion = 0,
        onSelect,
        onChanged,
        onBeforeDelete = null,
    } = $props();

    const tree = $derived(buildTree(folders));
    const byId = $derived(new Map(folders.map((f) => [f.id, f])));
    const rows = $derived(flattenTree(tree.roots, collapsed));
    const recoveredRows = $derived(flattenTree(tree.recovered, collapsed));

    let rootExpanded = $state(true);
    let rootAnimating = $state(false);
    let expandedIds = $state(new Set());
    const collapsed = $derived(new Set(folders.filter((folder) => !expandedIds.has(folder.id)).map((folder) => folder.id)));
    let menuAnchor = $state.raw(null);
    let openMenu = $state(null); // folder id with its ⋯ menu open
    let editing = $state(null); // { mode, id?, parentId? }
    let nameValue = $state('');
    let moveTarget = $state('root');
    let treeNote = $state('');
    let pendingDeletion = $state.raw(null);

    $effect(() => {
        void revealVersion;
        untrack(() => {
            setRootExpanded(true);
            if (selection.kind === 'folder') {
                expandedIds = new Set([...expandedIds, ...folderPathIds(folders, selection.id)]);
            }
        });
    });

    // Screen copy comes from the local table; unknown codes use the
    // single shared rule instead of leaking the raw IPC code.
    function describeError(e) {
        return resolveErrorText(FOLDER_TREE_ERRORS, e).text;
    }

    function isSelected(id) {
        return selection.kind === 'folder' && selection.id === id;
    }

    function selectFolder(id) {
        expandedIds = new Set([...expandedIds, id]);
        if (isSelected(id)) return;
        onSelect?.({ kind: 'folder', id, includeDescendants: false });
    }

    function setRootExpanded(expanded) {
        if (rootExpanded === expanded) return;
        rootAnimating = !window.matchMedia('(prefers-reduced-motion: reduce)').matches;
        rootExpanded = expanded;
        if (!expanded) closeMenu();
    }

    function finishRootAnimation(event) {
        if (event.target === event.currentTarget && event.propertyName === 'grid-template-rows') rootAnimating = false;
    }

    function selectRoot() {
        setRootExpanded(true);
        onSelect?.({ kind: 'root' });
    }

    function toggle(id) {
        const next = new Set(expandedIds);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        expandedIds = next;
    }

    function closeMenu() {
        openMenu = null;
        menuAnchor = null;
    }

    function toggleMenu(id, anchor) {
        if (openMenu === id) {
            closeMenu();
            return;
        }
        editing = null;
        menuAnchor = anchor;
        openMenu = id;
    }

    function selectMenuAction(action) {
        const folder = byId.get(openMenu);
        if (!folder) return closeMenu();
        if (action === 'create') openCreate(folder.id);
        else if (action === 'rename') openRename(folder);
        else if (action === 'move') openMove(folder);
        else if (action === 'delete') openDelete(folder);
    }

    function closeForms() {
        editing = null;
        closeMenu();
    }

    function openCreate(parentId) {
        setRootExpanded(true);
        if (parentId) expandedIds = new Set([...expandedIds, parentId]);
        nameValue = '';
        treeNote = '';
        editing = { mode: 'create', parentId };
        closeMenu();
    }

    function openRename(folder) {
        nameValue = folder.name;
        treeNote = '';
        editing = { mode: 'rename', id: folder.id };
        closeMenu();
    }

    function openMove(folder) {
        moveTarget = folder.parent_id ?? 'root';
        treeNote = '';
        editing = { mode: 'move', id: folder.id };
        closeMenu();
    }

    function openDelete(folder) {
        treeNote = '';
        editing = null;
        pendingDeletion = { id: folder.id, rev: folder.rev, name: folder.name, returnFocus: menuAnchor };
        closeMenu();
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
        const folder = pendingDeletion;
        if (!folder) return;
        if ((await onBeforeDelete?.()) === false) throw 'folder_delete_save_failed';
        await api.folderDelete(folder.id, folder.rev);
    }

    function deletionConfirmed() {
        pendingDeletion = null;
        treeNote = '';
        onChanged?.({ folderDeleted: true });
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
            <button
                {@attach (node) => {
                    if (isSelected(f.id)) node.scrollIntoView({ block: 'nearest' });
                }}
                class="folder-name" aria-current={isSelected(f.id) ? 'location' : undefined} onclick={() => selectFolder(f.id)}>
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
                aria-haspopup="menu"
                aria-expanded={openMenu === f.id}
                onclick={(event) => toggleMenu(f.id, event.currentTarget)}>
                ⋯
            </button>
        </div>
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
    </li>
{/snippet}

<div class="folder-tree">
    <div class="folder-row root-row" class:active={selection.kind !== 'folder'}>
        <button
            class="disclosure"
            aria-expanded={rootExpanded}
            aria-controls="root-folders"
            aria-label={rootExpanded ? 'Collapse ROOT' : 'Expand ROOT'}
            onclick={() => setRootExpanded(!rootExpanded)}>
            {rootExpanded ? '▾' : '▸'}
        </button>
        <button
            class="folder-name root-name"
            aria-current={selection.kind !== 'folder' ? 'location' : undefined}
            onclick={selectRoot}>ROOT</button>
        <button class="new-root" aria-label="New folder at the vault root" onclick={() => openCreate(null)}>
            +
        </button>
    </div>
    <div
        class="root-children"
        class:closed={!rootExpanded}
        class:animating={rootAnimating}
        id="root-folders"
        inert={!rootExpanded}
        aria-hidden={!rootExpanded}
        ontransitionend={finishRootAnimation}>
        <div class="root-children-clip">
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
                    {@render folderRow(node, depth + 1, false)}
                {/each}
            </ul>

            {#if recoveredRows.length > 0}
                <p class="recovered-head">Recovered</p>
                <ul class="tree">
                    {#each recoveredRows as { node, depth } (node.folder.id)}
                        {@render folderRow(node, depth + 1, true)}
                    {/each}
                </ul>
            {/if}
        </div>
    </div>
</div>

{#if openMenu && menuAnchor && byId.has(openMenu)}
    <AnchoredMenu
        anchor={menuAnchor}
        actions={FOLDER_ACTIONS}
        label={`Actions for ${byId.get(openMenu).name}`}
        onSelect={selectMenuAction}
        onClose={closeMenu} />
{/if}

{#if pendingDeletion}
    <ConfirmationModalContainer
        {...FOLDER_DELETE_MODAL}
        body={folderDeleteBody(pendingDeletion.name)}
        returnFocus={pendingDeletion.returnFocus}
        onConfirm={deleteFolder}
        onConfirmed={deletionConfirmed}
        onCancel={() => (pendingDeletion = null)}
        describeError={describeError} />
{/if}

<style src="./styles.css"></style>
