// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

export function entryLocationScope(entry, folders) {
    if (entry.folder_id == null) return { kind: 'root' };
    if (folders.some((folder) => folder.id === entry.folder_id)) {
        return { kind: 'folder', id: entry.folder_id, includeDescendants: false };
    }
    return { kind: 'all' };
}

export function folderPathIds(folders, id) {
    const byId = new Map(folders.map((folder) => [folder.id, folder]));
    const seen = new Set();
    const path = [];
    while (id != null && byId.has(id) && !seen.has(id)) {
        seen.add(id);
        path.unshift(id);
        id = byId.get(id).parent_id;
    }
    return path;
}
