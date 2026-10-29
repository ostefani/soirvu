// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
/**
 * What the Touch ID card shows for a given backend status.
 *
 * `note` is a warning, `description` plain text, `action` the one button.
 * `action.handler` is a key into `handlers`; `action.kind` is omitted when the
 * Button default should apply.
 *
 * @typedef {{
 *   note?: string,
 *   description?: string,
 *   action?: { label: string, handler: string, kind?: string },
 * }} TouchIdView
 *
 * @typedef {{ capable?: boolean, enrolled?: boolean, built_in?: boolean }} TouchIdStatus
 */

/**
 * The card is only offered when Touch ID is usable or already enrolled.
 * (Settings should show no dead options.)
 *
 * @param {TouchIdStatus | null | undefined} status
 */
export function isTouchIdOffered(status) {
    return Boolean(status?.capable || status?.enrolled);
}

/**
 * @param {TouchIdStatus | null | undefined} status
 * @param {Record<string, string>} text
 * @returns {TouchIdView | null}
 */
export function getTouchIdView(status, text) {
    if (!status) return null;

    if (status.enrolled && !status.capable) {
        const reason = status.built_in ? text.touchIdNotSignedProfile : text.touchIdNotBuilt;
        return {
            note: `${text.touchIdEnrolledNotCapableLead} ${reason}${text.touchIdEnrolledNotCapableTail}`,
            action: { label: text.touchIdRemoveEnrollment, kind: 'quiet', handler: 'touchIdDisable' },
        };
    }

    if (!status.capable && status.built_in) {
        return { description: text.touchIdNotSigned };
    }

    if (status.enrolled) {
        return {
            description: text.touchIdEnrolled,
            action: { label: text.touchIdTurnOff, kind: 'quiet', handler: 'touchIdDisable' },
        };
    }

    if (status.capable) {
        return {
            description: text.touchIdCapable,
            action: { label: text.touchIdTurnOn, handler: 'touchIdEnable' },
        };
    }

    return null;
}
