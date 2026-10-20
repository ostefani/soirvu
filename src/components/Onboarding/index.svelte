<script>
    import Button from '@/components/Button/index.svelte';
    import Input from '@/components/Input/index.svelte';
    import ErrorComponent from '@/components/ErrorComponent/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import Checkbox from '@/components/Checkbox/index.svelte';
    import { ONBOARDING_STEP } from '@/containers/OnboardingContainer/constants.js';

    let { model = $bindable(), fields, actions, onboardingText } = $props();
</script>

<div class="center-col">
    {#if model.step === ONBOARDING_STEP.CHOOSE_FOLDER}
        <NoteComponent text={onboardingText.step1Description} />
        {#each actions.pick as action (action.name)}
            <Button {...action} />
        {/each}
    {:else if model.step === ONBOARDING_STEP.SET_PASSPHRASE}
        <p class="folder-selection">
            <span class="folder-label">{onboardingText.folderLabel}</span>
            <strong class="folder-name">{model.folderName}</strong>
        </p>
        <NoteComponent variant="warning" text={model.warning} />
        {#each fields as field (field.name)}
            <Input {field} bind:value={model[field.name]} />
        {/each}
        {#each actions.create as action (action.name)}
            <Button {...action} />
        {/each}
    {:else}
        <NoteComponent
            variant="warning"
            title={onboardingText.recoveryKeyNoticeTitle}
            text={onboardingText.recoveryKeyNotice}
        />
        <div class="recovery-container">
            <p class="recovery-key">{model.recoveryKey}</p>
        </div>
        <Checkbox bind:checked={model.saved} label={onboardingText.confirmSavedCheckbox} />
        {#each actions.done as action (action.name)}
            <Button {...action} />
        {/each}
    {/if}

    {#if model.step < ONBOARDING_STEP.SAVE_RECOVERY_KEY}
        {#each actions.cancel as action (action.name)}
            <Button {...action} />
        {/each}
    {/if}

    <ErrorComponent text={model.error} />
</div>

<style src="./styles.css"></style>
