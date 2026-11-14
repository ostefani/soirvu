// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// S5, executable form: entry-target identity and URL classification
// live in a dependency-free leaf — no Markdown parser, Svelte or
// IPC — so the parser gate cannot form an import cycle with the
// analysis module that consumes it.

import { test } from "node:test";
import assert from "node:assert/strict";

import { classifyHref, isEntryHref, normalizeWebHref, parseEntryTarget } from "../entry-target.js";

const A = "01900000-0000-7000-8000-000000000001";

test("canonical destinations validate; paths and suffixes do not", () => {
    assert.equal(parseEntryTarget(`entry:${A}`), A);
    assert.equal(parseEntryTarget(`entry:${A.toUpperCase()}`), A);
    assert.equal(parseEntryTarget(`entry:${A}extra`), null);
    assert.equal(parseEntryTarget(`entry:${A}/more`), null);
    assert.equal(parseEntryTarget(`entry://${A}`), null);
    assert.equal(parseEntryTarget(`entry:${A}?x=1`), null);
    assert.equal(parseEntryTarget(`entry:${A}#frag`), null);
    assert.equal(parseEntryTarget(`entry:not-a-uuid`), null);
    assert.equal(parseEntryTarget("https://example.com"), null);
    assert.equal(parseEntryTarget(null), null);
    assert.ok(isEntryHref(`entry:${A}`));
    assert.ok(!isEntryHref(`entry:${A}extra`));
});

test("classification never broadens external permissions", () => {
    assert.equal(classifyHref(`entry:${A}`), "entry");
    assert.equal(classifyHref("https://example.com/x"), "external");
    assert.equal(classifyHref("mailto:a@b.c"), "external");
    assert.equal(classifyHref("attachment:abc"), "attachment");
    assert.equal(classifyHref("javascript:alert(1)"), "other");
    assert.equal(classifyHref("data:text/plain,x"), "other");
    assert.equal(classifyHref(`entry:${A}extra`), "other");
    assert.equal(classifyHref(""), "other");
});

test('web destinations are normalized and bounded before opening', () => {
    assert.equal(normalizeWebHref('https://münich.example/help'), 'https://xn--mnich-kva.example/help');
    assert.equal(normalizeWebHref('https://example.com/%E2%80%8B'), null);
    assert.equal(normalizeWebHref('https://example.com/%E2%80%AE'), null);
    const prefix = 'https://example.com/';
    const atLimit = prefix + 'a'.repeat(4096 - prefix.length);
    assert.equal(normalizeWebHref(atLimit), atLimit);
    assert.equal(normalizeWebHref(atLimit + 'a'), null);
    assert.equal(normalizeWebHref('https://user@example.com'), null);
});

test('a literal percent is allowed while encoded format characters stay blocked', () => {
    assert.equal(normalizeWebHref('https://example.com/50%off'), 'https://example.com/50%off');
    assert.equal(normalizeWebHref('https://example.com/50%off/%E2%80%8D'), null);
});

test('legacy percent escapes with invalid UTF-8 fail closed', () => {
    assert.equal(normalizeWebHref('https://example.com/caf%E9'), null);
});
