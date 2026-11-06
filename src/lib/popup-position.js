// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

export function anchoredPopupPosition(anchor, popup, viewport, margin = 8) {
    const width = Math.min(popup.width, Math.max(0, viewport.width - margin * 2));
    const height = Math.min(popup.height, Math.max(0, viewport.height - margin * 2));
    const below = viewport.height - anchor.bottom - margin;
    const above = anchor.top - margin;
    const preferredTop = below < height && above > below ? anchor.top - height : anchor.bottom;
    return {
        left: Math.max(margin, Math.min(anchor.right - width, viewport.width - width - margin)),
        top: Math.max(margin, Math.min(preferredTop, viewport.height - height - margin)),
    };
}
