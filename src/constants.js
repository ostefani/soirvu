/**
 * Application-level domain constants.
 *
 * Defines screens, view modes, vault status flags, and keyboard shortcuts.
 */

export const APPLICATION_SCREEN = {
    BOOT: 'boot',
    MAIN: 'main',
    DIARY: 'diary',
    SETTINGS: 'settings',
};

export const MAIN_VIEW = {
    WELCOME: 'welcome',
    LOCKED: 'locked',
};

export const VAULT_STATUS = {
    UNLOCKED: 'unlocked',
};

export const KEYBOARD_SHORTCUT = {
    LOCK_KEY: 'l',
};

export const EVENT_TYPE = {
    CREATED: 'created',
    DELETED: 'deleted',
    UPDATED: 'updated',
    SAVED: 'saved',
    COPIED: 'copied',
    ERROR: 'error',
    INFO: 'info',
    WARNING: 'warning',
    NEUTRAL: 'neutral',
    SUCCESS: 'success',
    DANGER: 'danger',
};
