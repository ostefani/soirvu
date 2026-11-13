import { ENTRY_EDITOR_TEXT as TEXT } from '../data.js';
import { attachmentLabel } from '../labels.js';

const ASSET_PREFIXES = ['soirvu-asset://localhost/', 'http://soirvu-asset.localhost/'];

export function assetIdFromSrc(src) {
    const prefix = ASSET_PREFIXES.find((p) => src.startsWith(p));
    return prefix ? src.slice(prefix.length) : null;
}

function media(tag, src) {
    const element = document.createElement(tag);
    element.controls = true;
    element.src = src;
    return element;
}

function preview(meta, urlFor) {
    if (meta.previewable) {
        if (meta.mime.startsWith('video/')) return media('video', urlFor(meta.id));
        if (meta.mime.startsWith('audio/')) return media('audio', urlFor(meta.id));
    }
    const label = document.createElement('span');
    label.textContent = attachmentLabel(meta.name, meta.size);
    return label;
}

export async function renderAttachment(el, id, attachments) {
    el.dataset.attachmentLabel ??= el.textContent;
    try {
        const meta = await attachments.info(id);
        el.replaceChildren(preview(meta, attachments.url));
        el.title = TEXT.doubleClickToOpen;
        el.ondblclick = () => attachments.open(meta.id);
    } catch {
        const label = el.dataset.attachmentLabel;
        el.textContent = label ? `${label} — ${TEXT.attachmentUnavailable}` : TEXT.attachmentUnavailable;
        el.title = TEXT.attachmentUnavailable;
        el.ondblclick = null;
    }
}
