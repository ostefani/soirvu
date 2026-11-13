export const SAVE_LABELS = {
    idle: '',
    dirty: 'Not saved yet',
    saving: 'Saving…',
    saved: 'Saved',
    error: 'Could not save',
};

export const SESSION_TEXT = {
    deleteFailed: 'Could not delete the entry. Your edits are kept.',
    deleteSaveFailed: 'Could not delete the entry — latest edits could not be saved. Your edits are kept.',
    linkMoved: 'The link moved — place the cursor on it and retry.',
    linkCopied: 'Internal link copied.',
    linkCopyFailed: 'Could not copy the link.',
    targetDeleted: 'The target entry was deleted.',
    diaryLocked: 'The diary is locked.',
    tooManyUploads: 'Too many files are being attached. Please try again.',
    latestEditsLost: 'Latest edits could not be saved.',
    navigateAfterSave: 'Latest edits could not be saved. Retry to navigate.',
    staleAttach:
        'latest edits could not be saved — attach again after saving.',
    foreignAttach: 'the entry changed elsewhere — reopen it and attach again.',
    tooLargeAttach: 'that file is too large for its type.',
    attachmentOpenFailed: 'Could not open the attachment.',
    attachmentOpenUnsupported: 'This attachment type cannot be opened. You can retrieve it through a diary export.',
    unsupportedAttach: 'that file type is not supported.',
};
