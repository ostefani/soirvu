<script>
    import Button from '@/components/Button/index.svelte';
    import Input from '@/components/Input/index.svelte';
    import ErrorComponent from '@/components/ErrorComponent/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import Checkbox from '@/components/Checkbox/index.svelte';
    import { ONBOARDING_STEP } from '@/containers/OnboardingContainer/constants.js';

    let {
        step,
        folderName,
        warning,
        recoveryKey,
        saved,
        busy,
        error,
        fields,
        values,
        onboardingText,
        onFieldInput,
        onSavedChange,
        onPick,
        onCreate,
        onDone,
        onCancel = null,
    } = $props();
</script>

<div class="center-col">
    {#if step === ONBOARDING_STEP.CHOOSE_FOLDER}
        <NoteComponent text={onboardingText.step1Description} />
        <Button label={onboardingText.chooseFolderButton} handler={onPick} />
    {:else if step === ONBOARDING_STEP.SET_PASSPHRASE}
        <p class="folder-selection">
            <span class="folder-label">{onboardingText.folderLabel}</span>
            <strong class="folder-name">{folderName}</strong>
        </p>
        <NoteComponent variant="warning" text={warning} />
        {#each fields as field (field.name)}
            <Input {field} value={values[field.name]} oninput={(e) => onFieldInput(field.name, e.target.value)} />
        {/each}
        <Button label={onboardingText.createDiaryButton} handler={onCreate} disabled={busy} />
    {:else if step === ONBOARDING_STEP.SAVE_RECOVERY_KEY}
        <NoteComponent
            variant="warning"
            title={onboardingText.recoveryKeyNoticeTitle}
            text={onboardingText.recoveryKeyNotice} />
        <div class="recovery-container">
            <p class="recovery-key">{recoveryKey}</p>
        </div>
        <Checkbox checked={saved} label={onboardingText.confirmSavedCheckbox} onchange={onSavedChange} />
        <Button label={onboardingText.openDiaryButton} handler={onDone} disabled={!saved} />
    {/if}

    {#if onCancel && step < ONBOARDING_STEP.SAVE_RECOVERY_KEY}
        <Button label={onboardingText.backButton} kind="quiet" handler={onCancel} />
    {/if}

    <ErrorComponent text={error} />
</div>

<style src="./styles.css"></style>
