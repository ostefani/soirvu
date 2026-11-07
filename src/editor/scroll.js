// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

export function caretScrollDelta(caret, viewport, margin = 12) {
    const inset = Math.min(margin, Math.max(0, (viewport.bottom - viewport.top - (caret.bottom - caret.top)) / 2));
    if (caret.bottom > viewport.bottom - inset) return caret.bottom - viewport.bottom + inset;
    if (caret.top < viewport.top + inset) return caret.top - viewport.top - inset;
    return 0;
}
