<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy } from 'svelte';
    import * as api from '@/api.js';
    import Onboarding from '@/components/Onboarding/index.svelte';

    import {
        createModel,
        ONBOARDING_STEP,
        MINIMUM_PASSPHRASE_LENGTH,
        PASSPHRASE_FIELD_CONFIGURATIONS,
    } from './constants.js';
    import { onboardingText } from './data.js';

    let { onDone, onCancel = null } = $props();

    let model = $state(createModel());
    // Completions must not repopulate a screen that is already gone.
    let alive = true;

    const fields = PASSPHRASE_FIELD_CONFIGURATIONS.map((field) => ({
        ...field,
        label: field.name === 'passphrase' ? onboardingText.passphraseLabel : onboardingText.confirmPassphraseLabel,
    }));

    function describeError(error) {
        return onboardingText.errors[error] ?? onboardingText.unknownError(error);
    }

    async function pick() {
        model.error = '';
        try {
            const picked = await api.vaultPickFolder('create');
            if (!alive || !picked) return;
            model.folderName = picked.name;
            model.warning = picked.warning ? (onboardingText.warnings[picked.warning] ?? '') : '';
            model.step = ONBOARDING_STEP.SET_PASSPHRASE;
        } catch (error) {
            if (!alive) return;
            model.error = describeError(error);
        }
    }

    async function create() {
        if (model.busy) return;
        model.error = '';
        if (model.passphrase.length < MINIMUM_PASSPHRASE_LENGTH) {
            model.error = onboardingText.passphraseTooShortError;
            return;
        }
        if (model.passphrase !== model.confirm) {
            model.error = onboardingText.passphraseMismatchError;
            return;
        }
        model.busy = true;
        try {
            const created = await api.vaultCreate(model.passphrase);
            if (!alive) return;
            model.recoveryKey = created.recovery_key;
            model.passphrase = '';
            model.confirm = '';
            model.step = ONBOARDING_STEP.SAVE_RECOVERY_KEY;
        } catch (error) {
            if (!alive) return;
            model.error = describeError(error);
        } finally {
            model.busy = false;
        }
    }

    // Don't leave secrets sitting in memory once the screen is gone.
    onDestroy(() => {
        alive = false;
        model.recoveryKey = '';
        model.passphrase = '';
        model.confirm = '';
    });

    const values = $derived({ passphrase: model.passphrase, confirm: model.confirm });

    function setField(name, value) {
        model[name] = value;
    }

    function setSaved(checked) {
        model.saved = checked;
    }
</script>

<Onboarding
    step={model.step}
    folderName={model.folderName}
    warning={model.warning}
    recoveryKey={model.recoveryKey}
    saved={model.saved}
    busy={model.busy}
    error={model.error}
    {fields}
    {values}
    {onboardingText}
    onFieldInput={setField}
    onSavedChange={setSaved}
    onPick={pick}
    onCreate={create}
    {onDone}
    {onCancel} />
