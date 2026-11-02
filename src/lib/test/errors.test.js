// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// S5, executable form: backend error codes resolve through one shared
// classification — known codes keep their screen copy without leaking the
// raw code, and every unknown code falls back to the single tested rule.

import { test } from "node:test";
import assert from "node:assert/strict";

import {
    ERROR_SEVERITY,
    classifyError,
    resolveErrorText,
    unknownErrorText,
} from "../errors.js";
import { ENTRY_LIST_ERRORS } from "../../components/EntryList/errors.js";
import { DIARY_ERRORS } from "../../containers/DiaryScreenContainer/errors.js";
import { FOLDER_TREE_ERRORS } from "../../components/FolderTree/errors.js";
import {
    SETTINGS_BACKUP_ERRORS,
    SETTINGS_EXPORT_ERRORS,
    SETTINGS_GENERAL_ERRORS,
    SETTINGS_SECURITY_ERRORS,
    SETTINGS_SYNC_ERRORS,
} from "../../containers/SettingsScreenContainer/errors.js";

/** Every code the UI handles, with its expected severity. */
const EXPECTED_SEVERITY = {
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
};

test("classifyError keeps known codes and flags unknown ones once", () => {
    for (const [code, severity] of Object.entries(EXPECTED_SEVERITY)) {
        assert.deepEqual(classifyError(code), { key: code, severity });
    }
    assert.deepEqual(classifyError("folder_ghost"), {
        key: "unknown",
        severity: ERROR_SEVERITY.UNKNOWN,
    });
    assert.deepEqual(classifyError(undefined), {
        key: "unknown",
        severity: ERROR_SEVERITY.UNKNOWN,
    });
});

test("unknownErrorText is the single fallback rule", () => {
    assert.equal(unknownErrorText("folder_ghost"), "Something went wrong (folder_ghost).");
});

test("entry list and diary never leak invalid_input", () => {
    for (const table of [ENTRY_LIST_ERRORS, DIARY_ERRORS]) {
        const { text } = resolveErrorText(table, "invalid_input");
        assert.ok(text.length > 0);
        assert.ok(!text.includes("invalid_input"));
    }
});

test("FolderTree resolves known codes and unknown codes once", () => {
    for (const [code, message] of Object.entries(FOLDER_TREE_ERRORS)) {
        assert.equal(resolveErrorText(FOLDER_TREE_ERRORS, code).text, message);
    }
    const code = "folder_ghost";
    assert.equal(resolveErrorText(FOLDER_TREE_ERRORS, code).text, unknownErrorText(code));
});

test("settings tables resolve their codes and fall back once", () => {
    const tables = [
        SETTINGS_SECURITY_ERRORS,
        SETTINGS_EXPORT_ERRORS,
        SETTINGS_SYNC_ERRORS,
        SETTINGS_BACKUP_ERRORS,
        SETTINGS_GENERAL_ERRORS,
    ];
    for (const table of tables) {
        for (const [code, message] of Object.entries(table)) {
            assert.equal(resolveErrorText(table, code).text, message);
        }
    }
    const code = "sync_ghost";
    assert.equal(
        resolveErrorText(SETTINGS_SYNC_ERRORS, code).text,
        unknownErrorText(code),
    );
});
