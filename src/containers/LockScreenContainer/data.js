/**
 * LockScreenContainer — UI text data and user-visible messages.
 *
 * All user-facing strings are defined here without abbreviations.
 * Dynamic messages use formatting helper functions.
 */
export const lockScreenText = {
    // Vault status badges and labels
    vaultFolderNotFound: '— folder not found',
    vaultVolumeNotMounted: '— volume not mounted',
    locateButton: 'Locate…',
    retryButton: 'Retry',
    forgetButton: 'Forget',
    forgetConfirmButton: 'Really forget?',

    // Unlock form actions and placeholders
    unlockButton: 'Unlock',
    touchIdButton: 'Unlock with Touch ID',
    passphrasePlaceholder: 'Passphrase',
    recoveryPlaceholder: 'XXXX-XXXX-…',

    // Fork (copy import) workflow
    forkDoneTitle: 'The copy is now a separate diary. Its new recovery key:',
    forkDoneNote: 'Save it now — it is shown once. The old recovery key opens only the original diary.',
    forkDoneButton: 'Done',
    /** @param {string} name */
    forkPrompt: (name) =>
        `"${name}" is a copy of a diary you already have. To keep both, the copy needs its own identity.`,
    forkButton: 'Make it a separate diary',
    forkCancelButton: 'Not now',

    // Navigation and folder management
    openAnotherButton: 'Open another diary folder…',
    createNewButton: 'Create a new diary…',

    // Rate limiting backoff error handling
    backoffPrefix: 'backoff:',
    /** @param {string} seconds */
    backoffMessage: (seconds) =>
        `Too many wrong attempts. Wait ${seconds} seconds and try again.`,

    // Fallback error message
    /** @param {string} code */
    unknownError: (code) => `Something went wrong (${code}).`,
};
