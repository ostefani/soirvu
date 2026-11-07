// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//
// Entry-list copy for backend error codes. Only codes with a curated
// message live here; everything else falls back to the shared unknown
// rule, so raw IPC codes never reach the list.

export const ENTRY_LIST_ERRORS = {
    locked: "The vault is locked.",
    invalid_input: "That search was not accepted.",
    storage: "A storage problem (storage).",
};
