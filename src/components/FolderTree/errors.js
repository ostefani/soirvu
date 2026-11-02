// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//
// Folder-tree copy for backend error codes. Only codes with a curated
// message live here; everything else falls back to the shared unknown
// rule, so raw IPC codes never reach the tree.

export const FOLDER_TREE_ERRORS = {
    locked: "The vault is locked.",
    folder_exists: "A folder with that name already exists here.",
    folder_not_found: "That folder is gone.",
    folder_not_empty: "Only empty folders can be removed.",
    folder_cycle: "A folder cannot go inside itself or its subfolders.",
    folder_depth_exceeded: "Folders cannot nest that deep.",
    revision_conflict: "That folder changed elsewhere — try again.",
    invalid_input: "That name was not accepted.",
    storage: "A storage problem (storage).",
};
