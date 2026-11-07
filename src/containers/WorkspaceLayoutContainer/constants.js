// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

export const PANE_MIN = { folders: 160, entries: 220 };
export const COLLAPSED_WIDTH = 40;
export const MAIN_MIN = 320;
export const DIVIDERS_WIDTH = 24;

export function createModel() {
    return {
        width: 1100,
        folders: { width: 220, collapsed: false },
        entries: { width: 280, collapsed: false },
    };
}

export function paneWidths(model) {
    const folders = model.folders.collapsed ? COLLAPSED_WIDTH : model.folders.width;
    const entries = model.entries.collapsed ? COLLAPSED_WIDTH : model.entries.width;
    const folderMin = model.folders.collapsed ? COLLAPSED_WIDTH : PANE_MIN.folders;
    const entryMin = model.entries.collapsed ? COLLAPSED_WIDTH : PANE_MIN.entries;
    const budget = Math.max(folderMin + entryMin, model.width - MAIN_MIN - DIVIDERS_WIDTH);
    const extra = Math.max(0, folders + entries - budget);
    const available = folders + entries - folderMin - entryMin;
    const ratio = available > 0 ? Math.min(1, extra / available) : 0;
    return {
        folders: folders - (folders - folderMin) * ratio,
        entries: entries - (entries - entryMin) * ratio,
    };
}
