// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

export const FOLDER_ACTIONS = [
    { id: 'create', label: 'New subfolder' },
    { id: 'rename', label: 'Rename' },
    { id: 'move', label: 'Move' },
    { id: 'delete', label: 'Remove' },
];


export const FOLDER_DELETE_MODAL = {
    title: 'Delete folder?',
    confirmLabel: 'Delete folder and contents',
    cancelLabel: 'Keep folder',
    pendingLabel: 'Deleting…',
    warning: 'All notes, attachments, and subfolders inside this folder will be deleted. This cannot be undone in the app.',
    tone: 'danger',
};

export function folderDeleteBody(name) {
    return `Delete “${name}” and everything inside it?`;
}
