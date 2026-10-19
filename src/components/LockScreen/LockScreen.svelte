<script>
    import Button from '../Button/Button.svelte';
    import Input from '../Input/Input.svelte';
    import ErrorComponent from '../ErrorComponent/ErrorComponent.svelte';
    import NoteComponent from '../NoteComponent/NoteComponent.svelte';

    let { vaults, selected, model = $bindable(), secretField, forkField, kindOptions, unlockActions, forkActions, handlers } = $props();
</script>
<style src="./LockScreen.css"></style>

<div class="center-col">
    <div class="vault-list">
        {#each vaults as vault (vault.id)}
            <div class="vault-row">
                <button
                    class="vault-name"
                    class:active={selected?.id === vault.id}
                    onclick={() => handlers.select(vault)}>
                    {vault.name}
                    {#if vault.state === 'not_found'}<span class="missing"> — folder not found</span>{/if}
                    {#if vault.state === 'volume_missing'}<span class="missing"> — volume not mounted</span>{/if}
                </button>
                {#if vault.state === 'not_found'}
                    <Button label="Locate…" handler={() => handlers.relocate(vault)} />
                {/if}
                {#if vault.state === 'volume_missing'}
                    <Button label="Retry" handler={() => handlers.refresh()} />
                {/if}
                {#if model.forgetArmed === vault.id}
                    <Button label="Really forget?" handler={() => handlers.forget(vault)} />
                {:else}
                    <Button label="Forget" kind="quiet" handler={() => handlers.armForget(vault)} />
                {/if}
            </div>
        {/each}
    </div>

    {#if selected && selected.state === 'available'}
        <div class="kind-toggle">
            {#each kindOptions as option (option.name)}
                <Button label={option.label} active={model.kind === option.name} handler={option.handler} />
            {/each}
        </div>
        <Input field={secretField} bind:value={model.secret} onenter={handlers.unlock} />
        {#each unlockActions as action (action.name)}
            <Button {...action} />
        {/each}
    {/if}

    {#if model.forkKey}
        <div class="fork-panel">
            <p>The copy is now a separate diary. Its new recovery key:</p>
            <p><code>{model.forkKey}</code></p>
            <p class="note">Save it now — it is shown once. The old recovery key opens only the original diary.</p>
            <Button label="Done" handler={handlers.finishFork} />
        </div>
    {:else if model.forkDest}
        <div class="fork-panel">
            <p>
                “{model.forkDest}” is a copy of a diary you already have. To keep both, the copy needs its own identity.
            </p>
            <Input field={forkField} bind:value={model.forkSecret} onenter={handlers.doFork} />
            {#each forkActions as action (action.name)}
                <Button {...action} />
            {/each}
            <ErrorComponent text={model.forkError} />
        </div>
    {/if}

    <Button label="Open another diary folder…" kind="quiet" handler={handlers.openAnother} />
    <Button label="Create a new diary…" kind="quiet" handler={handlers.create} />

    <NoteComponent text={model.note} />
    <ErrorComponent text={model.error} />
</div>
