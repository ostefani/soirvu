import { normalizeTags } from '../../lib/tags.js';

export function createDraft({ onEdit }) {
    let title = $state('');
    let entryDate = $state('');
    let mood = $state('');
    let tags = $state.raw([]);
    let body = $state('');

    return {
        get title() {
            return title;
        },
        set title(value) {
            title = value;
            onEdit();
        },
        get entryDate() {
            return entryDate;
        },
        set entryDate(value) {
            entryDate = value;
            onEdit();
        },
        get mood() {
            return mood;
        },
        set mood(value) {
            mood = value;
            onEdit();
        },
        get tags() {
            return tags;
        },
        set tags(value) {
            tags = value;
            onEdit();
        },
        get body() {
            return body;
        },
        set body(value) {
            body = value;
            onEdit();
        },

        load(entry) {
            title = entry.title ?? '';
            entryDate = entry.entry_date;
            mood = entry.mood == null ? '' : String(entry.mood);
            tags = normalizeTags(entry.tags);
            body = entry.body_md;
        },

        snapshot(tz) {
            const trimmed = title.trim();
            return {
                entry_date: entryDate,
                title: trimmed === '' ? null : trimmed,
                body_md: body,
                mood: mood === '' ? null : Number(mood),
                tz,
                tags: [...tags],
            };
        },
    };
}
