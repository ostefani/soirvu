// SPDX-License-Identifier: MIT
const TEXT_INPUT_TYPES = new Set(['text', 'search', 'url', 'tel', 'email', 'password']);

export function editMenuRequest(event, activeElement, selection) {
    const target = event.target;
    const field = target?.closest?.('input, textarea');
    const isTextField = field && (field.tagName === 'TEXTAREA' || TEXT_INPUT_TYPES.has(field.type));
    const editable = isTextField
        ? field === activeElement && !field.disabled && !field.readOnly
        : Boolean(target?.isContentEditable && activeElement?.contains(target));
    const copy = isTextField
        ? field === activeElement && field.type !== 'password' && field.selectionEnd > field.selectionStart
        : Boolean(target && selection && !selection.isCollapsed && selection.containsNode(target, true));
    if (!copy && !editable) return null;

    const keyboard = event.clientX === 0 && event.clientY === 0;
    const bounds = keyboard ? target.getBoundingClientRect() : null;
    return {
        x: Math.max(0, Math.round(bounds?.left ?? event.clientX)),
        y: Math.max(0, Math.round(bounds?.bottom ?? event.clientY)),
        copy,
        cut: copy && editable,
        paste: editable,
    };
}
