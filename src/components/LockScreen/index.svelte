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

    // Position of the segmented thumb for the kind toggle.
    const kindIndex = $derived(
        Math.max(
            0,
            kindOptions.findIndex((o) => o.name === model.kind),
        ),
    );
</script>

<div class="center-col">
    <div class="vault-list" role="radiogroup" aria-label="Select vault">
        {#each vaults as vault (vault.id)}
            <div class="vault-row">
                <label class="vault-pick">
                    <input
                        class="vault-radio"
                        type="radio"
                        name="vault-pick"
                        value={vault.id}
                        checked={selected?.id === vault.id}
                        onchange={() => handlers.select(vault)} />
                    <span class="radio-dot" aria-hidden="true"></span>
                    <svg
                        class="vault-icon"
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true">
                        <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                        <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
                    </svg>
                    <span class="vault-name">
                        {vault.name}
                        {#if vault.state === VAULT_STATE.NOT_FOUND}
                            <span class="missing"> {textData.vaultFolderNotFound}</span>
                        {/if}
                        {#if vault.state === VAULT_STATE.VOLUME_MISSING}
                            <span class="missing"> {textData.vaultVolumeNotMounted}</span>
                        {/if}
                    </span>
                </label>
                <div class="vault-actions">
                    {#if vault.state === VAULT_STATE.NOT_FOUND}
                        <Button label={textData.locateButton} handler={() => handlers.relocate(vault)} />
                    {/if}
                    {#if vault.state === VAULT_STATE.VOLUME_MISSING}
                        <Button label={textData.retryButton} handler={handlers.refresh} />
                    {/if}
                    {#if model.forgetArmed === vault.id}
                        <Button label={textData.forgetConfirmButton} handler={() => handlers.forget(vault)} />
                        <Button
                            label={textData.forgetCancelButton ?? 'Cancel'}
                            kind="quiet"
                            handler={() => (model.forgetArmed = null)} />
                    {:else}
                        <Button
                            label={textData.forgetButton}
                            kind="texture-button"
                            handler={() => handlers.armForget(vault)} />
                    {/if}
                </div>
            </div>
        {/each}
    </div>

    {#if selected && selected.state === VAULT_STATE.AVAILABLE}
        <div class="kind-toggle" role="radiogroup" aria-label="Unlock with">
            <span
                class="kind-thumb"
                aria-hidden="true"
                style="width: {100 / kindOptions.length}%; transform: translateX({kindIndex * 100}%)"></span>
            {#each kindOptions as option (option.name)}
                <label class="kind-option" class:active={model.kind === option.name}>
                    <input
                        class="kind-radio"
                        type="radio"
                        name="auth-kind"
                        value={option.name}
                        checked={model.kind === option.name}
                        onchange={option.handler} />
                    <span class="kind-label">{option.label}</span>
                </label>
            {/each}
        </div>
        <Input
            field={secretField}
            value={model.secret}
            oninput={(e) => handlers.setSecret(e.currentTarget.value)}
            onenter={handlers.unlock} />
        {#each unlockActions as action (action.name)}
            <Button kind="primary-button" {...action} />
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
            <Input
                field={forkField}
                value={model.forkSecret}
                oninput={(e) => handlers.setForkSecret(e.currentTarget.value)}
                onenter={handlers.doFork} />
            {#each forkActions as action (action.name)}
                <Button {...action} />
            {/each}
            <ErrorComponent text={model.forkError} />
        </div>
    {/if}

    <Button label={textData.openAnotherButton} kind="texture-button" handler={handlers.openAnother} />
    <Button label={textData.createNewButton} kind="texture-button" handler={handlers.create} />

    <NoteComponent text={model.note} />
    <ErrorComponent text={model.error} />
</div>

<style src="./styles.css"></style>
