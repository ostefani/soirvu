// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

/**
 * Calculates how far to scroll to bring the caret into view.
 * @param {{top: number, bottom: number}} caret Caret bounds in viewport coordinates.
 * @param {{top: number, bottom: number}} viewport Visible scroll bounds in the same coordinates.
 * @param {number} [margin=12] Preferred clearance in CSS pixels.
 * @returns {number} Signed CSS pixels to add to scrollTop, or zero when the caret is already in view.
 */
export function caretScrollDelta(caret, viewport, margin = 12) {
    const inset = Math.min(margin, Math.max(0, (viewport.bottom - viewport.top - (caret.bottom - caret.top)) / 2));
    if (caret.bottom > viewport.bottom - inset) return caret.bottom - viewport.bottom + inset;
    if (caret.top < viewport.top + inset) return caret.top - viewport.top - inset;
    return 0;
}
