/**
 * OnboardingContainer — Domain constants and model factory.
 */

export const ONBOARDING_STEP = {
    CHOOSE_FOLDER: 1,
    SET_PASSPHRASE: 2,
    SAVE_RECOVERY_KEY: 3,
};

export const MINIMUM_PASSPHRASE_LENGTH = 8;

export function createModel() {
    return {
        step: ONBOARDING_STEP.CHOOSE_FOLDER,
        folderName: '',
        warning: '',
        passphrase: '',
        confirm: '',
        recoveryKey: '',
        saved: false,
        error: '',
        busy: false,
    };
}

export const PASSPHRASE_FIELD_CONFIGURATIONS = [
    {
        name: 'passphrase',
        label: 'Passphrase',
        type: 'password',
        autocomplete: 'new-password',
    },
    {
        name: 'confirm',
        label: 'Repeat it',
        type: 'password',
        autocomplete: 'new-password',
    },
];

export const ONBOARDING_ERROR_CODES = {
    FOLDER_NOT_EMPTY: 'folder_not_empty',
    EMPTY_PASSPHRASE: 'empty_passphrase',
    NO_FOLDER_PICKED: 'no_folder_picked',
    STORAGE: 'storage',
};

export const FILESYSTEM_WARNING_CODES = {
    CLOUD_SYNC: 'cloud_sync',
    NETWORK_FS: 'network_fs',
};
