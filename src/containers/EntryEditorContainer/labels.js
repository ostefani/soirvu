import { SESSION_TEXT } from './data.js';

export function untitledForDate(date) {
    return `Untitled — ${date}`;
}

export function defaultLinkLabel(target) {
    return target.title || untitledForDate(target.entry_date);
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
    if (code === 'revision_conflict') return attachError(name, SESSION_TEXT.foreignAttach);
    if (code === 'too_large') return attachError(name, SESSION_TEXT.tooLargeAttach);
    if (code === 'unsupported_type') return attachError(name, SESSION_TEXT.unsupportedAttach);
    if (code === 'locked') return SESSION_TEXT.diaryLocked;
    if (code === 'too_many_uploads') return SESSION_TEXT.tooManyUploads;
    return attachError(name, `could not attach the file (${code}).`);
}

export function linkTargetError(reason) {
    return `Could not read the target (${reason}).`;
}
