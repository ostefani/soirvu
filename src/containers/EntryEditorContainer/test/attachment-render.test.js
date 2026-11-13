import { test } from 'node:test';
import assert from 'node:assert/strict';
import { renderAttachment } from '../../../components/EntryEditor/EditorSurface/attachments.js';
import { ENTRY_EDITOR_TEXT } from '../../../components/EntryEditor/data.js';

test('repeated metadata failures preserve the original attachment label', async () => {
    const el = { dataset: {}, textContent: 'Voice memo' };
    const attachments = { info: async () => { throw 'storage'; } };
    await renderAttachment(el, 'id', attachments);
    await renderAttachment(el, 'id', attachments);
    assert.equal(el.textContent, `Voice memo — ${ENTRY_EDITOR_TEXT.attachmentUnavailable}`);
    assert.equal(el.ondblclick, null);
});

test('an attachment above its preview limit keeps its external open action', async () => {
    const previous = globalThis.document;
    globalThis.document = { createElement: (tag) => ({ tag }) };
    try {
        const el = {
            dataset: {},
            textContent: 'Clip',
            replaceChildren(child) { this.child = child; },
        };
        const opened = [];
        await renderAttachment(el, 'id', {
            info: async () => ({ id: 'id', mime: 'video/mp4', name: 'Clip', size: 5, previewable: false }),
            url: () => assert.fail('a label should not request a preview URL'),
            open: (id) => opened.push(id),
        });
        assert.equal(el.child.tag, 'span');
        assert.equal(el.title, ENTRY_EDITOR_TEXT.doubleClickToOpen);
        el.ondblclick();
        assert.deepEqual(opened, ['id']);
    } finally {
        globalThis.document = previous;
    }
});
