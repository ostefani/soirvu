// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//
// Shared backend-error classification. Screens own their copy in local
// `errors.js` tables; this module only decides which key a backend code
// maps to, how severe it is, and the single rule for codes no table
// knows. No per-code user strings live here — only keys — so each
// added code is one table row plus one test case.

/** How the UI should treat a failure: fix the input, reload and retry, retry later, or report it. */
export const ERROR_SEVERITY = {
    VALIDATION: "validation",
    CONFLICT: "conflict",
    UNAVAILABLE: "unavailable",
    UNKNOWN: "unknown",
};

/** Backend code → severity for every code the UI handles. */
const ERROR_SEVERITY_BY_CODE = {
    passphrase_too_short: ERROR_SEVERITY.VALIDATION,
    passphrase_too_long: ERROR_SEVERITY.VALIDATION,
    invalid_input: ERROR_SEVERITY.VALIDATION,
    query_too_short: ERROR_SEVERITY.VALIDATION,
    too_large: ERROR_SEVERITY.VALIDATION,
    unsupported_type: ERROR_SEVERITY.VALIDATION,
    too_many_uploads: ERROR_SEVERITY.VALIDATION,
    wrong_passphrase: ERROR_SEVERITY.VALIDATION,
    invalid_recovery_key: ERROR_SEVERITY.VALIDATION,
    not_a_vault: ERROR_SEVERITY.VALIDATION,
    destination_not_empty: ERROR_SEVERITY.VALIDATION,
    export_target_exists: ERROR_SEVERITY.VALIDATION,
    folder_exists: ERROR_SEVERITY.VALIDATION,
    folder_not_empty: ERROR_SEVERITY.VALIDATION,
    folder_cycle: ERROR_SEVERITY.VALIDATION,
    folder_depth_exceeded: ERROR_SEVERITY.VALIDATION,
    revision_conflict: ERROR_SEVERITY.CONFLICT,
    vault_mismatch: ERROR_SEVERITY.CONFLICT,
    sync_shared_identity: ERROR_SEVERITY.CONFLICT,
    locked: ERROR_SEVERITY.UNAVAILABLE,
    storage: ERROR_SEVERITY.UNAVAILABLE,
    rotation_recovery_needed: ERROR_SEVERITY.UNAVAILABLE,
    corrupt: ERROR_SEVERITY.UNAVAILABLE,
    vault_not_found: ERROR_SEVERITY.UNAVAILABLE,
    touch_id_unavailable: ERROR_SEVERITY.UNAVAILABLE,
    sync_unreadable: ERROR_SEVERITY.UNAVAILABLE,
    sync_folder_missing: ERROR_SEVERITY.UNAVAILABLE,
    sync_not_configured: ERROR_SEVERITY.UNAVAILABLE,
    sync: ERROR_SEVERITY.UNAVAILABLE,
    backup_unavailable: ERROR_SEVERITY.UNAVAILABLE,
    entry_not_found: ERROR_SEVERITY.UNAVAILABLE,
    folder_not_found: ERROR_SEVERITY.UNAVAILABLE,
    folder_delete_save_failed: ERROR_SEVERITY.UNAVAILABLE,
};

/**
 * Classifies a backend error code. Known codes keep their code as the
 * table key; anything else — including non-strings — resolves to the
 * single `unknown` key, never the raw code.
 */
export function classifyError(code) {
    const key = String(code);
    if (Object.hasOwn(ERROR_SEVERITY_BY_CODE, key)) {
        return { key, severity: ERROR_SEVERITY_BY_CODE[key] };
    }
    return { key: "unknown", severity: ERROR_SEVERITY.UNKNOWN };
}

/**
 * The single unknown-code rule: labels the failure and keeps the code
 * for bug reports instead of leaking it as the message.
 */
export function unknownErrorText(code) {
    return `Something went wrong (${code}).`;
}

/**
 * Resolves screen copy for a backend error. Known codes read their
 * table entry; unknown codes use the single fallback rule.
 */
export function resolveErrorText(table, code) {
    const { key, severity } = classifyError(code);
    const text = Object.hasOwn(table, key) ? table[key] : unknownErrorText(code);
    return { text, severity };
}
