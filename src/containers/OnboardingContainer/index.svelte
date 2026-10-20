<script>
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

    function describeError(error) {
        return onboardingText.errors[error] ?? onboardingText.unknownError(error);
    }

    async function pick() {
        model.error = '';
        const picked = await api.vaultPickFolder('create');
        if (picked) {
            model.folderName = picked.name;
            model.warning = picked.warning ? (onboardingText.warnings[picked.warning] ?? '') : '';
            model.step = ONBOARDING_STEP.SET_PASSPHRASE;
        }
    }

    async function create() {
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
            model.recoveryKey = created.recovery_key;
            model.passphrase = '';
            model.confirm = '';
            model.step = ONBOARDING_STEP.SAVE_RECOVERY_KEY;
        } catch (error) {
            model.error = describeError(error);
        } finally {
            model.busy = false;
        }
    }

    const fields = PASSPHRASE_FIELD_CONFIGURATIONS.map((field) => ({
        ...field,
        label: field.name === 'passphrase'
            ? onboardingText.passphraseLabel
            : onboardingText.confirmPassphraseLabel,
    }));

    const actions = $derived({
        pick: [
            {
                name: 'pick',
                label: onboardingText.chooseFolderButton,
                handler: pick,
            },
        ],
        create: [
            {
                name: 'create',
                label: onboardingText.createDiaryButton,
                handler: create,
                disabled: model.busy,
            },
        ],
        done: [
            {
                name: 'done',
                label: onboardingText.openDiaryButton,
                handler: () => onDone(),
                disabled: !model.saved,
            },
        ],
        cancel: onCancel
            ? [
                {
                    name: 'cancel',
                    label: onboardingText.backButton,
                    kind: 'quiet',
                    handler: () => onCancel(),
                },
            ]
            : [],
    });
</script>

<Onboarding bind:model {fields} {actions} {onboardingText} />
