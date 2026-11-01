// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// IPC payload regression test: Tauri converts only top-level
// command arguments to camelCase; nested struct fields
// deserialize strictly. Sending a snake_case top-level key
// rejects the call (`missing required key`), and a camelCase
// nested key rejects it (`missing field`) — both failed loudly
// in front of the user (unsavable entries, unscoped lists).
// This test captures exactly what crosses invoke() and pins
// every multi-word key on both levels.

import { test } from "node:test";
import assert from "node:assert/strict";

const calls = [];
globalThis.window = {
    __TAURI_INTERNALS__: {
        invoke: async (cmd, args) => {
            calls.push({ cmd, args });
            return null;
        },
    },
};

const api = await import("../../api.js");

const DRAFT = {
    entry_date: "2026-11-01",
    title: null,
    body_md: "x",
    mood: null,
    tz: null,
    tags: [],
};

function last(cmd) {
    const found = calls.filter((c) => c.cmd === cmd).at(-1);
    assert.ok(found, `expected an invoke of ${cmd}`);
    return found.args;
}

test("entry writes use camelCase top-level keys", async () => {
    await api.entryCreate("folder-1", DRAFT);
    assert.deepEqual(last("entry_create"), { folderId: "folder-1", draft: DRAFT });
    await api.entryCreate(null, DRAFT);
    assert.deepEqual(last("entry_create"), { folderId: null, draft: DRAFT });
    await api.entryUpdate("entry-1", 3, DRAFT);
    assert.deepEqual(last("entry_update"), { id: "entry-1", expectedRev: 3, draft: DRAFT });
});

test("folder writes use camelCase top-level keys", async () => {
    await api.folderCreate("parent-1", "Name");
    assert.deepEqual(last("folder_create"), { parentId: "parent-1", name: "Name" });
    await api.folderCreate(null, "Name");
    assert.deepEqual(last("folder_create"), { parentId: null, name: "Name" });
    await api.folderUpdate("f1", 2, "parent-1", "Name");
    assert.deepEqual(last("folder_update"), {
        id: "f1",
        expectedRev: 2,
        parentId: "parent-1",
        name: "Name",
    });
    await api.folderDelete("f1", 2);
    assert.deepEqual(last("folder_delete"), { id: "f1", expectedRev: 2 });
    await api.folderRestore("f1", 2);
    assert.deepEqual(last("folder_restore"), { id: "f1", expectedRev: 2 });
});

test("scoped reads carry the snake_case nested flag", async () => {
    const folder = { kind: "folder", id: "f1", includeDescendants: true };
    await api.entryList({ scope: folder });
    assert.deepEqual(last("entry_list").scope, {
        kind: "folder",
        id: "f1",
        include_descendants: true,
    });
    await api.calendarDays("2026-11-01", "2026-12-01", folder);
    assert.deepEqual(last("calendar_days").scope, {
        kind: "folder",
        id: "f1",
        include_descendants: true,
    });
    await api.search("query", { scope: folder });
    assert.deepEqual(last("search").scope, {
        kind: "folder",
        id: "f1",
        include_descendants: true,
    });
    await api.tagList(folder);
    assert.deepEqual(last("tag_list").scope, {
        kind: "folder",
        id: "f1",
        include_descendants: true,
    });
    await api.tagList(null);
    assert.equal(last("tag_list").scope, null);
});

test("pre-existing camelCase top-level keys are unchanged", async () => {
    await api.vaultUnlock("vault-1", "passphrase", "secret");
    assert.deepEqual(last("vault_unlock"), {
        vaultId: "vault-1",
        kind: "passphrase",
        secret: "secret",
    });
    await api.attachmentBegin("entry-1", "a.bin", null);
    assert.deepEqual(last("attachment_begin"), {
        entryId: "entry-1",
        name: "a.bin",
        mimeHint: null,
    });
    await api.attachmentChunk("upload-1", new Uint8Array([1, 2, 3]));
    const chunk = last("attachment_chunk");
    assert.equal(chunk.uploadId, "upload-1");
    assert.equal(typeof chunk.dataBase64, "string");
});
