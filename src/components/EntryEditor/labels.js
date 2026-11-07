import { ENTRY_EDITOR_TEXT } from './data.js';

export function moodChipLabel(mood) {
    return mood === '' ? ENTRY_EDITOR_TEXT.moodChip : `Mood ${mood}/5`;
}

export function tagsChipLabel(tags) {
    if (tags.length === 0) return ENTRY_EDITOR_TEXT.tagsChip;
    if (tags.length === 1) return tags[0];
    return `${tags[0]} +${tags.length - 1}`;
}

export function attachmentLabel(name, size) {
    return `${name} · ${Math.max(1, Math.round(size / 1024))} KB`;
}

export function loadProblemText(problem) {
    if (problem === 'deleted') return ENTRY_EDITOR_TEXT.loadDeleted;
    if (problem === 'missing') return ENTRY_EDITOR_TEXT.loadMissing;
    return ENTRY_EDITOR_TEXT.loadError;
}
