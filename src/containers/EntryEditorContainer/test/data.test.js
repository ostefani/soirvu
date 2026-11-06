import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
    ENTRY_EDITOR_TEXT,
    LINK_POPOVER_ACTIONS,
    MOOD_OPTIONS,
    SAVE_LABELS,
    TOOLBAR_CODE_BLOCK,
    TOOLBAR_FORMAT_COMMANDS,
    TOOLBAR_LIST_COMMANDS,
    TOOLBAR_TABLE_COMMANDS,
    TOOLBAR_TEXT,
    TOOLBAR_TYPE_COMMANDS,
    attachError,
    attachmentLabel,
    attachmentStatus,
    linkTargetError,
    loadProblemText,
    moodChipLabel,
    tagsChipLabel,
    untitledForDate,
    uploadFailureText,
} from '../data.js';
import { ATTACH_CHUNK_SIZE, AUTOSAVE_DELAY, MAX_CONCURRENT_UPLOADS } from '../constants.js';

test('save labels cover every autosave state', () => {
    assert.deepEqual(Object.keys(SAVE_LABELS).sort(), ['dirty', 'error', 'idle', 'saved', 'saving']);
    assert.equal(SAVE_LABELS.idle, '');
});

test('mood options render the 1-5 scale in a loop', () => {
    assert.equal(MOOD_OPTIONS.length, 5);
    assert.deepEqual(
        MOOD_OPTIONS.map((option) => option.value),
        ['1', '2', '3', '4', '5'],
    );
    for (const option of MOOD_OPTIONS) assert.equal(option.label, option.value);
});

test('link popover actions carry unique ids and labels', () => {
    const ids = LINK_POPOVER_ACTIONS.map((action) => action.id);
    assert.equal(new Set(ids).size, ids.length);
    for (const action of LINK_POPOVER_ACTIONS) assert.match(action.label, /\S/);
});

test('attachment status prefers the pending count over failures', () => {
    assert.equal(attachmentStatus(1, []), 'Attaching 1 file…');
    assert.equal(attachmentStatus(2, ['a']), 'Attaching 2 files…');
    assert.equal(attachmentStatus(0, ['a']), 'a');
    assert.equal(attachmentStatus(0, ['a', 'b']), '2 files could not be attached. a');
    assert.equal(attachmentStatus(0, []), '');
});

test('upload failures name the file and the reason', () => {
    assert.equal(uploadFailureText('a.png', 'locked'), ENTRY_EDITOR_TEXT.diaryLocked);
    assert.equal(uploadFailureText('a.png', 'too_many_uploads'), ENTRY_EDITOR_TEXT.tooManyUploads);
    assert.match(uploadFailureText('a.png', 'too_large'), /a\.png.*too large/);
    assert.match(uploadFailureText('a.png', 'unsupported_type'), /a\.png.*not supported/);
    assert.match(uploadFailureText('a.png', 'revision_conflict'), /a\.png.*reopen it/);
    assert.match(uploadFailureText('a.png', 'boom'), /a\.png.*boom/);
    assert.equal(attachError('a.png', 'x'), 'a.png: x');
});

test('chip and label helpers format values without side effects', () => {
    assert.equal(moodChipLabel(''), ENTRY_EDITOR_TEXT.moodChip);
    assert.equal(moodChipLabel('3'), 'Mood 3/5');
    assert.equal(tagsChipLabel([]), ENTRY_EDITOR_TEXT.tagsChip);
    assert.equal(tagsChipLabel(['a']), 'a');
    assert.equal(tagsChipLabel(['a', 'b']), 'a +1');
    assert.equal(tagsChipLabel(['a', 'b', 'c']), 'a +2');
    assert.equal(untitledForDate('2026-10-20'), 'Untitled — 2026-10-20');
    assert.equal(attachmentLabel('a.png', 2048), 'a.png · 2 KB');
    assert.match(linkTargetError('gone'), /gone/);
});

test('load problems resolve to a message', () => {
    assert.equal(loadProblemText('deleted'), ENTRY_EDITOR_TEXT.loadDeleted);
    assert.equal(loadProblemText('missing'), ENTRY_EDITOR_TEXT.loadMissing);
    assert.equal(loadProblemText('error'), ENTRY_EDITOR_TEXT.loadError);
});

test('toolbar groups carry unique ids, labels, and active predicates', () => {
    const groups = [TOOLBAR_FORMAT_COMMANDS, TOOLBAR_TYPE_COMMANDS, TOOLBAR_LIST_COMMANDS, TOOLBAR_TABLE_COMMANDS];
    const ids = groups.flat().map((command) => command.id);
    assert.equal(new Set(ids).size, ids.length);
    for (const command of groups.flat()) {
        assert.match(command.text, /\S/);
        assert.match(command.title, /\S/);
        assert.equal(typeof command.active, 'function');
    }
    assert.match(TOOLBAR_TEXT.toolbarLabel, /\S/);
    assert.match(TOOLBAR_CODE_BLOCK.text, /\S/);
    const strong = TOOLBAR_FORMAT_COMMANDS[0];
    assert.equal(strong.active({ strong: true }), true);
    assert.equal(strong.active({}), false);
    const paragraph = TOOLBAR_TYPE_COMMANDS[0];
    assert.equal(paragraph.active({ block: 'paragraph', list: null }), true);
    assert.equal(paragraph.active({ block: 'paragraph', list: 'bullet' }), false);
    const tableOnly = TOOLBAR_TABLE_COMMANDS.filter((command) => command.inTableOnly);
    assert.ok(tableOnly.length > 0);
});

test('behavioral constants stay positive', () => {
    assert.ok(MAX_CONCURRENT_UPLOADS > 0);
    assert.ok(ATTACH_CHUNK_SIZE > 0);
    assert.ok(AUTOSAVE_DELAY > 0);
});
