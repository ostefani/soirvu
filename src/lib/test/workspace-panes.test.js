// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from 'node:test';
import assert from 'node:assert/strict';
import {
    createModel,
    paneWidths,
    MAIN_MIN,
    DIVIDERS_WIDTH,
    COLLAPSED_WIDTH,
    PANE_MIN,
} from '../../containers/WorkspaceLayoutContainer/constants.js';

test('resizing the window preserves readable panes and main content', () => {
    const model = createModel();
    model.folders.width = 900;
    model.entries.width = 1200;
    for (const width of [761, 900, 1100, 1600, 2400]) {
        model.width = width;
        const sizes = paneWidths(model);
        assert.ok(sizes.folders >= PANE_MIN.folders);
        assert.ok(sizes.entries >= PANE_MIN.entries);
        assert.ok(width - sizes.folders - sizes.entries - DIVIDERS_WIDTH >= MAIN_MIN - 0.001);
    }
});

test('collapsing either pane frees space and restores its preferred width', () => {
    for (const pane of ['folders', 'entries']) {
        const model = createModel();
        const preferred = model[pane].width;
        model[pane].collapsed = true;
        assert.equal(paneWidths(model)[pane], COLLAPSED_WIDTH);
        model.width = 800;
        paneWidths(model);
        model.width = 1600;
        model[pane].collapsed = false;
        assert.equal(paneWidths(model)[pane], preferred);
    }
});

test('both panes can collapse without losing the saved dimensions', () => {
    const model = createModel();
    model.folders.collapsed = true;
    model.entries.collapsed = true;
    assert.deepEqual(paneWidths(model), { folders: COLLAPSED_WIDTH, entries: COLLAPSED_WIDTH });
    assert.equal(model.folders.width, 220);
    assert.equal(model.entries.width, 280);
});
