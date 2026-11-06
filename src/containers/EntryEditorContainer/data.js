

export const SAVE_LABELS = {
    idle: '',
    dirty: 'Not saved yet',
    saving: 'Saving…',
    saved: 'Saved',
    error: 'Could not save',
};

export const MOOD_OPTIONS = [
    { value: '1', label: '1' },
    { value: '2', label: '2' },
    { value: '3', label: '3' },
    { value: '4', label: '4' },
    { value: '5', label: '5' },
];

export const ENTRY_EDITOR_TEXT = {
    titlePlaceholder: 'Title',
    opening: 'Opening…',
    back: 'Back',
    goBackTitle: 'Go back',
    goForwardTitle: 'Go forward',
    goBackLabel: 'Go back to the previous entry',
    goForwardLabel: 'Go forward to the next entry',
    deleteEntry: 'Delete entry',
    confirmDelete: 'Really delete entry?',
    keepEntry: 'Keep it',
    deleteFailed: 'Could not delete the entry. Your edits are kept.',
    deleteSaveFailed: 'Could not delete the entry — latest edits could not be saved. Your edits are kept.',
    retry: 'Retry',
    dateChipFallback: 'Date',
    moodChip: 'Mood',
    tagsChip: 'Tags',
    dateHeading: 'Date',
    moodHeading: 'Mood',
    tagsHeading: 'Tags',
    linkedFrom: 'Linked from',
    copyNoteLink: 'Copy link',
    copyingLink: 'Copying…',
    clearMood: 'Clear mood',
    removeTagLabel: 'Remove tag',
    addTagPlaceholder: 'Add tag… (Enter)',
    attachmentUnavailable: 'Attachment unavailable.',
    doubleClickToOpen: 'Double-click to open',
    linkMoved: 'The link moved — place the cursor on it and retry.',
    linkCopied: 'Internal link copied.',
    linkCopyFailed: 'Could not copy the link.',
    targetDeleted: 'The target entry was deleted.',
    linkActionsLabel: 'Link actions',
    openExternalLink: 'Open link',
    diaryLocked: 'The diary is locked.',
    tooManyUploads: 'Too many files are being attached. Please try again.',
    latestEditsLost: 'Latest edits could not be saved.',
    navigateAfterSave: 'Latest edits could not be saved. Retry to navigate.',
    conflictMessage:
        'This entry changed elsewhere — your latest edits are kept but not saved. Reopen the entry to merge.',
    conflictStatus: 'Not saved — changed elsewhere',
    loadDeleted: 'This entry was deleted. Its links elsewhere keep their labels.',
    loadMissing: 'This entry is unavailable.',
    loadError: 'Could not open this entry.',
    staleAttach:
        'latest edits could not be saved — attach again after saving.',
    foreignAttach: 'the entry changed elsewhere — reopen it and attach again.',
    tooLargeAttach: 'that file is too large for its type.',
    unsupportedAttach: 'that file type is not supported.',
};

export const LINK_POPOVER_ACTIONS = [
    { id: 'open', label: 'Open entry', primary: true },
    { id: 'copy', label: 'Copy link', primary: false },
    { id: 'refresh', label: 'Use current title', primary: false },
    { id: 'remove', label: 'Remove link', primary: false },
];

export const TOOLBAR_TEXT = {
    toolbarLabel: 'Formatting',
    attach: 'Attach…',
    linkEntry: 'Link…',
    linkEntryTitle: 'Link selected text to an entry',
    languagePlaceholder: 'language',
};

export const TOOLBAR_FORMAT_COMMANDS = [
    { id: 'strong', text: 'B', title: 'Bold (Ctrl+B)', textClass: 'tb-bold', buttonClass: '', active: selectedFlag('strong') },
    { id: 'em', text: 'I', title: 'Italic (Ctrl+I)', textClass: 'tb-italic', buttonClass: '', active: selectedFlag('em') },
    { id: 'strike', text: 'S', title: 'Strikethrough (Ctrl+Shift+X)', textClass: '', buttonClass: 'tb-strike', active: selectedFlag('strike') },
    { id: 'code', text: '</>', title: 'Inline code (Ctrl+E)', textClass: '', buttonClass: '', active: selectedFlag('code') },
];

export const TOOLBAR_TYPE_COMMANDS = [
    { id: 'paragraph', text: '¶', title: 'Paragraph', textClass: '', buttonClass: '', active: (selection) => selection?.block === 'paragraph' && !selection?.list },
    { id: 'heading1', text: 'H1', title: 'Heading 1', textClass: '', buttonClass: '', active: (selection) => selection?.headingLevel === 1 },
    { id: 'heading2', text: 'H2', title: 'Heading 2', textClass: '', buttonClass: '', active: (selection) => selection?.headingLevel === 2 },
    { id: 'heading3', text: 'H3', title: 'Heading 3', textClass: '', buttonClass: '', active: (selection) => selection?.headingLevel === 3 },
];

export const TOOLBAR_LIST_COMMANDS = [
    { id: 'quote', text: '❝', title: 'Quote', textClass: '', buttonClass: '', active: selectedFlag('quote') },
    { id: 'bulletList', text: '• List', title: 'Bullet list', textClass: '', buttonClass: '', active: (selection) => selection?.list === 'bullet' },
    { id: 'orderedList', text: '1. List', title: 'Numbered list', textClass: '', buttonClass: '', active: (selection) => selection?.list === 'ordered' },
    { id: 'taskList', text: '☑ Task', title: 'Task list', textClass: '', buttonClass: '', active: (selection) => selection?.list === 'task' },
];

export const TOOLBAR_TABLE_COMMANDS = [
    { id: 'insertTable', text: 'Table', title: 'Insert table', textClass: '', buttonClass: '', inTableOnly: false, active: () => false },
    { id: 'addRow', text: '+Row', title: 'Add a row below', textClass: '', buttonClass: '', inTableOnly: true, active: () => false },
    { id: 'addColumn', text: '+Col', title: 'Add a column to the right', textClass: '', buttonClass: '', inTableOnly: true, active: () => false },
    { id: 'deleteRow', text: '−Row', title: 'Delete this row', textClass: '', buttonClass: '', inTableOnly: true, active: () => false },
    { id: 'deleteColumn', text: '−Col', title: 'Delete this column', textClass: '', buttonClass: '', inTableOnly: true, active: () => false },
];

export const TOOLBAR_CODE_BLOCK = { id: 'codeBlock', text: '{ }', title: 'Code block' };

function selectedFlag(name) {
    return (selection) => !!selection?.[name];
}

export function moodChipLabel(mood) {
    return mood === '' ? ENTRY_EDITOR_TEXT.moodChip : `Mood ${mood}/5`;
}

export function tagsChipLabel(tags) {
    if (tags.length === 0) return ENTRY_EDITOR_TEXT.tagsChip;
    if (tags.length === 1) return tags[0];
    return `${tags[0]} +${tags.length - 1}`;
}

export function untitledForDate(date) {
    return `Untitled — ${date}`;
}

export function defaultLinkLabel(target) {
    return target.title || untitledForDate(target.entry_date);
}

export function attachmentLabel(name, size) {
    return `${name} · ${Math.max(1, Math.round(size / 1024))} KB`;
}

export function attachmentStatus(pending, failures) {
    if (pending > 0) return `Attaching ${pending} file${pending === 1 ? '' : 's'}…`;
    if (failures.length === 1) return failures[0];
    if (failures.length > 1) return `${failures.length} files could not be attached. ${failures[0]}`;
    return '';
}

export function attachError(name, reason) {
    return `${name}: ${reason}`;
}

export function uploadFailureText(name, code) {
    if (code === 'revision_conflict') return attachError(name, ENTRY_EDITOR_TEXT.foreignAttach);
    if (code === 'too_large') return attachError(name, ENTRY_EDITOR_TEXT.tooLargeAttach);
    if (code === 'unsupported_type') return attachError(name, ENTRY_EDITOR_TEXT.unsupportedAttach);
    if (code === 'locked') return ENTRY_EDITOR_TEXT.diaryLocked;
    if (code === 'too_many_uploads') return ENTRY_EDITOR_TEXT.tooManyUploads;
    return attachError(name, `could not attach the file (${code}).`);
}

export function linkTargetError(reason) {
    return `Could not read the target (${reason}).`;
}

export function loadProblemText(problem) {
    if (problem === 'deleted') return ENTRY_EDITOR_TEXT.loadDeleted;
    if (problem === 'missing') return ENTRY_EDITOR_TEXT.loadMissing;
    return ENTRY_EDITOR_TEXT.loadError;
}
