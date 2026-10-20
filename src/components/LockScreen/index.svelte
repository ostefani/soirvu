<script>
    import Button from '@/components/Button/index.svelte';
    import Input from '@/components/Input/index.svelte';
    import ErrorComponent from '@/components/ErrorComponent/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import { VAULT_STATE } from '@/containers/LockScreenContainer/constants.js';

    let {
        vaults,
        selected,
        model = $bindable(),
        secretField,
        forkField,
        kindOptions,
        unlockActions,
        forkActions,
        handlers,
        textData,
    } = $props();
</script>

<div class="center-col">
    <div class="vault-list">
        {#each vaults as vault (vault.id)}
            <div class="vault-row">
                <button
                    class="vault-name"
                    class:active={selected?.id === vault.id}
                    onclick={() => handlers.select(vault)}>
                    {vault.name}
                    {#if vault.state === VAULT_STATE.NOT_FOUND}
                        <span class="missing"> {textData.vaultFolderNotFound}</span>
                    {/if}
                    {#if vault.state === VAULT_STATE.VOLUME_MISSING}
                        <span class="missing"> {textData.vaultVolumeNotMounted}</span>
                    {/if}
                </button>
                {#if vault.state === VAULT_STATE.NOT_FOUND}
                    <Button label={textData.locateButton} handler={() => handlers.relocate(vault)} />
                {/if}
                {#if vault.state === VAULT_STATE.VOLUME_MISSING}
                    <Button label={textData.retryButton} handler={() => handlers.refresh()} />
                {/if}
                {#if model.forgetArmed === vault.id}
                    <Button label={textData.forgetConfirmButton} handler={() => handlers.forget(vault)} />
                {:else}
                    <Button label={textData.forgetButton} kind="quiet" handler={() => handlers.armForget(vault)} />
                {/if}
            </div>
        {/each}
    </div>

    {#if selected && selected.state === VAULT_STATE.AVAILABLE}
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
            <p>{textData.forkDoneTitle}</p>
            <p><code>{model.forkKey}</code></p>
            <p class="note">{textData.forkDoneNote}</p>
            <Button label={textData.forkDoneButton} handler={handlers.finishFork} />
        </div>
    {:else if model.forkDest}
        <div class="fork-panel">
            <p>{textData.forkPrompt(model.forkDest)}</p>
            <Input field={forkField} bind:value={model.forkSecret} onenter={handlers.doFork} />
            {#each forkActions as action (action.name)}
                <Button {...action} />
            {/each}
            <ErrorComponent text={model.forkError} />
        </div>
    {/if}

    <Button label={textData.openAnotherButton} kind="quiet" handler={handlers.openAnother} />
    <Button label={textData.createNewButton} kind="quiet" handler={handlers.create} />

    <NoteComponent text={model.note} />
    <ErrorComponent text={model.error} />
</div>

<style src="./styles.css"></style>
