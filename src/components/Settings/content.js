// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
/**
 * Static content and structure of the Settings screen.
 *
 * Labels, titles, lists of things to render and layout decisions live here, so
 * index.svelte only holds state and composition. Text that the parent passes in
 * `settingsText` always wins over DEFAULT_TEXT.
 */

/** @typedef {{ id: string, label: string }} TabDef */

/** @type {TabDef[]} */
export const TABS = [
    { id: 'general', label: 'General' },
    { id: 'security', label: 'Security' },
    { id: 'backup', label: 'Backups' },
    { id: 'sync', label: 'Sync' },
    { id: 'transfer', label: 'Import & Export' },
];

/**
 * Strings that used to be hard-coded in the markup. Add the same key to
 * `settingsText` to override (or translate) one.
 *
 * @type {Record<string, string>}
 */
export const DEFAULT_TEXT = {
    storageTitle: 'Diary Storage & Inactivity Lock',
    storageIntro: 'View storage location and configure automatic locking.',
    privacyTitle: 'Privacy & Clipboard',
    privacyIntro: 'Control location metadata handling and clipboard auto-clearing.',
    backupTitle: 'Automated & Manual Backups',
    deviceIdentityTitle: 'Device Identity',
};

/**
 * Checkboxes in the Privacy card. `label` is a key into the text, `setting` a
 * key in `general.settings`, `handler` a key in `handlers`. `platform` limits
 * an option to one OS.
 *
 * @typedef {{ setting: string, label: string, handler: string, platform?: string }} PrivacyOption
 */

/** @type {PrivacyOption[]} */
export const PRIVACY_OPTIONS = [
    { setting: 'strip_exif', label: 'stripExif', handler: 'saveStrip' },
    { setting: 'clipboard_clear', label: 'clipboardClear', handler: 'saveClipboard', platform: 'macos' },
];

/** Security form: fields that start in the right-hand column ("Confirm new passphrase"). */
export const SECURITY_SECOND_COLUMN = [2];

/** Actions that get a button kind of their own; everything else is a texture button. */
export const ACTION_KINDS = { 'change-passphrase': 'primary-button' };

/**
 * `defaults` with every defined entry of `overrides` applied on top.
 * Unlike a spread, an explicit `undefined` does not erase a default.
 *
 * @param {Record<string, any>} defaults
 * @param {Record<string, any> | null | undefined} overrides
 */
export function withDefaults(defaults, overrides) {
    const merged = { ...defaults };
    for (const [key, value] of Object.entries(overrides ?? {})) {
        if (value !== undefined) merged[key] = value;
    }
    return merged;
}
