/**
 * OnboardingContainer — UI text data and messages.
 */

export const onboardingText = {
    // Step 1: Choose Folder
    step1Description: 'Create your diary in a folder you choose. Everything in it is encrypted.',
    chooseFolderButton: 'Choose an empty folder…',

    // Step 2: Set Passphrase
    folderLabel: 'Diary folder:',
    passphraseLabel: 'Passphrase',
    confirmPassphraseLabel: 'Repeat it',
    createDiaryButton: 'Create the diary',

    // Step 3: Save Recovery Key
    recoveryKeyNoticeTitle: 'Important: Recovery Key',
    recoveryKeyNotice:
        'This is your recovery key. If you forget the passphrase, it is the only way back in. The app does not keep a copy — write it down and keep it somewhere safe.',
    confirmSavedCheckbox: 'I have saved the recovery key',
    openDiaryButton: 'Open my diary',

    // Navigation
    backButton: 'Back',

    // Validation messages
    passphraseTooShortError:
        'Use at least 8 characters — longer is better. This passphrase is the only protection the diary has.',
    passphraseMismatchError: 'The two passphrases do not match.',

    // System error translations
    errors: {
        folder_not_empty:
            'That folder is not empty. A vault needs an empty folder — pick one, or create a new subfolder for it.',
        empty_passphrase: 'The passphrase must not be empty.',
        no_folder_picked: 'Pick a folder first.',
        storage: 'The vault could not be written there. Is the folder writable?',
    },

    // Filesystem warnings
    warnings: {
        cloud_sync:
            "This folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — a synced copy can be taken mid-write and sync conflicts can corrupt the vault. You can continue, but a local folder is safer.",
        network_fs:
            "This folder is on a network share. The diary's database (SQLite) is unreliable over network filesystems. You can continue, but a local folder is safer.",
    },

    /** @param {string} code */
    unknownError: (code) => `Something went wrong (${code}).`,
};
