/**
 * LockScreenContainer — Domain constants and model factory.
 *
 * Contains immutable domain shapes: vault states, authentication kinds,
 * field descriptors, error mappings, filesystem warnings, and initial state factory.
 * All text strings for user display are in data.js.
 */

// ---------------------------------------------------------------------------
// Vault lifecycle states
// ---------------------------------------------------------------------------
export const VAULT_STATE = {
    AVAILABLE: 'available',
    NOT_FOUND: 'not_found',
    VOLUME_MISSING: 'volume_missing',
    UNLOCKED: 'unlocked',
};

// ---------------------------------------------------------------------------
// Authentication kinds
// ---------------------------------------------------------------------------
export const AUTHENTICATION_KIND = {
    PASSPHRASE: 'passphrase',
    RECOVERY: 'recovery',
};

// ---------------------------------------------------------------------------
// Initial model shape — pure data, zero reactive wrapping.
// ---------------------------------------------------------------------------
export function createModel() {
    return {
        selectedId: null,
        kind: AUTHENTICATION_KIND.PASSPHRASE,
        secret: '',
        error: '',
        busy: false,
        forgetArmed: null,
        forkDest: null,
        forkSecret: '',
        forkKey: null,
        forkNewId: null,
        forkError: '',
        forkBusy: false,
        note: '',
        touchId: null,
    };
}

// ---------------------------------------------------------------------------
// Authentication kind toggle options
// ---------------------------------------------------------------------------
export const AUTHENTICATION_KIND_OPTIONS = [
    { name: AUTHENTICATION_KIND.PASSPHRASE, label: 'Passphrase' },
    { name: AUTHENTICATION_KIND.RECOVERY, label: 'Recovery key' },
];

// ---------------------------------------------------------------------------
// Static field configuration for the fork password input
// ---------------------------------------------------------------------------
export const FORK_SECRET_FIELD_CONFIGURATION = {
    name: 'forkSecret',
    type: 'password',
    placeholder: "The diary's passphrase",
};

// ---------------------------------------------------------------------------
// Error code → user message mapping
// ---------------------------------------------------------------------------
export const VAULT_ERROR_MESSAGES = {
    wrong_passphrase: 'Wrong passphrase or recovery key.',
    invalid_recovery_key: 'That does not look like a recovery key.',
    storage: 'The vault folder is not available right now.',
    vault_not_found: 'This vault is no longer in the list.',
    vault_mismatch: 'That folder holds a different vault.',
    touch_id_unavailable: 'Touch ID is not available for this diary right now.',
    destination_not_empty: 'That folder is not empty. Pick an empty folder for the restored diary.',
    corrupt: 'The vault looks damaged. The CLI can check it: soirvu-cli verify.',
};

// ---------------------------------------------------------------------------
// Filesystem warning code → advisory prose
// ---------------------------------------------------------------------------
export const FILESYSTEM_WARNING_MESSAGES = {
    cloud_sync:
        "That folder is in a cloud-synced location. Sync clients and the diary's database files do not mix — the vault is registered, but keeping it there risks corruption.",
    network_fs:
        "That folder is on a network share. The diary's database is unreliable over network filesystems — the vault is registered, but keeping it there is risky.",
};
