export const copy = {
    title: 'Settings',
    loading: 'Loading…',
    back: 'Back to the diary',
    vaultLocation: 'This diary lives in:',
    notSet: 'not set',
    backupLocation: 'Backup location:',
    backupNotes: [
        'A backup is an encrypted copy of this vault (the newest 7 are kept). To restore one, choose its folder from the lock screen.',
    ],
    stripExif: 'Strip location metadata (EXIF) from photos on import',
    clipboardClear: 'Clear the clipboard 30 seconds after copying from the diary',
    securityTitle: 'Passphrase and recovery key',
    securityNotes: [
        "Only the diary's header changes — nothing is re-encrypted and the diary stays open. Backups made earlier keep the passphrase and recovery key they were made with.",
    ],
    rotating:
        "Changing the master key… a fresh copy of the diary is being built and checked. This can take a while for a large diary — don't close the app.",
    newKeyLead: 'Your new recovery key:',
    newKeyNote: 'Save it now — it is shown once, and the old key no longer works.',
    touchIdTitle: 'Touch ID',
    touchIdRemoveEnrollment: 'Remove Touch ID enrollment',
    touchIdTurnOff: 'Turn off Touch ID',
    touchIdTurnOn: 'Turn on Touch ID',
    touchIdEnrolled: 'This diary can be unlocked with Touch ID on this Mac.',
    touchIdCapable:
        'Unlock this diary with your fingerprint instead of typing the passphrase. The passphrase and recovery key keep working — Touch ID is one more key, never the only one.',
    touchIdNotSigned:
        'Touch ID needs this app signed with a development profile that carries the keychain entitlement. This build is not signed that way, so Touch ID is unavailable — the passphrase and recovery key work as always.',
    touchIdEnrolledNotCapableLead: 'This diary has Touch ID enrolled, but this build cannot use it — it',
    touchIdNotSignedProfile: 'is not signed with the keychain profile',
    touchIdNotBuilt: 'was made without Touch ID',
    touchIdEnrolledNotCapableTail:
        ". The passphrase and recovery key work as always. You can remove the enrollment here; removing it needs no keychain, only the diary's header changes.",
    exportTitle: 'Export',
    exportIntro:
        'Export writes the diary as plain, unencrypted files. You will be asked to confirm, to type your passphrase again, and to pick an empty folder.',
    exportNotes: [
        'Markdown is for reading and moving to other apps. JSON keeps everything — ids, deleted entries — and is made for importing back into Soirvu.',
    ],
    importTitle: 'Import',
    importIntro:
        'Import adds entries to this diary — nothing is overwritten or merged. A Markdown folder can be a Soirvu export or notes from another app. A JSON file must be a Soirvu export; entries whose id already exists are skipped.',
    syncTitle: 'Sync',
    syncIntro:
        "Sync writes this diary's changes as encrypted bundles into a folder. Another tool — Syncthing, a cloud folder, a USB stick — carries the folder between your devices; Soirvu itself never uses the network.",
    syncFolder: 'Sync folder:',
    ackAhead:
        'An ack in this folder has claimed more than this vault holds — a copy may share its identity, or this vault was restored. Entries a peer pruned on the strength of it return when that peer re-emits everything. If THIS diary is the copy, restamp it below; if it is the original, the copy is the one to restamp. Once the missing entries are back, dismiss this note.',
    dismiss: 'Dismiss',
    pruneNotes: [
        'Prune removes the bundles and blobs every known peer has already consumed. A device that uses this folder but has not synced yet — a fresh copy, for instance — can lose changes it never received. Prune only once every device has synced.',
    ],
    restampIntro:
        "If this diary is a copy of a folder from another device, both copies share one device identity and cannot sync with each other. Restamping gives THIS copy a new identity — the other device is untouched, and this copy's changes are emitted again under the new identity.",
};

export const fields = {
    timeout: { name: 'timeoutMin', label: 'Lock after (minutes of inactivity)', type: 'number', min: 1 },
    security: [
        { name: 'curPass', label: 'Current passphrase', type: 'password' },
        { name: 'newPass', label: 'New passphrase', type: 'password' },
        { name: 'newPass2', label: 'Confirm new passphrase', type: 'password' },
    ],
    exportPass: { name: 'pass', label: 'Passphrase', type: 'password', autocomplete: 'current-password' },
    schedule: {
        name: 'schedule',
        label: 'Automatic backups',
        options: [
            { value: 'off', label: 'Off' },
            { value: 'daily', label: 'Daily' },
            { value: 'weekly', label: 'Weekly' },
        ],
    },
};

