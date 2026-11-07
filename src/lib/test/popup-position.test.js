// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

import test from 'node:test';
import assert from 'node:assert/strict';
import { anchoredPopupPosition } from '../popup-position.js';

const viewport = { width: 900, height: 600 };
const popup = { width: 160, height: 150 };

test('folder actions open below the trigger when there is room', () => {
    assert.deepEqual(anchoredPopupPosition({ top: 100, bottom: 125, right: 220 }, popup, viewport), { left: 60, top: 125 });
});

test('folder actions near the bottom open upward and remain fully visible', () => {
    const point = anchoredPopupPosition({ top: 550, bottom: 575, right: 220 }, popup, viewport);
    assert.equal(point.top, 400);
    assert.ok(point.top + popup.height < viewport.height);
});

test('popup placement stays inside horizontal viewport edges', () => {
    assert.equal(anchoredPopupPosition({ top: 100, bottom: 125, right: 30 }, popup, viewport).left, 8);
    assert.equal(anchoredPopupPosition({ top: 100, bottom: 125, right: 950 }, popup, viewport).left, 732);
});

test('oversized menus fit short viewports and can scroll within their height cap', () => {
    assert.deepEqual(anchoredPopupPosition({ top: 20, bottom: 45, right: 100 }, popup, { width: 120, height: 100 }), { left: 8, top: 8 });
});
