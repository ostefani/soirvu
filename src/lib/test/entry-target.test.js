// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// S5, executable form: entry-target identity and URL classification
// live in a dependency-free leaf — no Markdown parser, Svelte or
// IPC — so the parser gate cannot form an import cycle with the
// analysis module that consumes it.

import { test } from "node:test";
import assert from "node:assert/strict";

import { classifyHref, isEntryHref, parseEntryTarget } from "../entry-target.js";

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
