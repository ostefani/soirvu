// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Step 8, executable form: only a canonical UUID after `entry:`
// routes in-vault, and only real link marks count — code, images,
// raw blocks, titles and near-miss destinations contribute
// nothing to the outgoing set.

import { test } from "node:test";
import assert from "node:assert/strict";

import { parseMarkdown, serializeMarkdown } from "../../editor/index.js";
import {
    classifyHref,
    clearBacklinkCache,
    isEntryHref,
    linksToTarget,
    outgoingEntryLinks,
    outgoingEntryRefs,
    parseEntryTarget,
    verifyBacklinkCandidates,
} from "../links.js";

const A = "01900000-0000-7000-8000-000000000001";
const B = "01900000-0000-7000-8000-000000000002";

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

test("canonical links round-trip through the editor serialization", () => {
    const src = `Worked on [SoirVu feature ideas](entry:${A}).\n\nCompleted [Training notes](entry:${B} "Training").\n`;
    const out = serializeMarkdown(parseMarkdown(src));
    assert.ok(out.includes(`(entry:${A})`), out);
    assert.ok(out.includes(`(entry:${B}`), out);
});

test("outgoing links ignore code, images, raw blocks and titles", () => {
    const src = [
        `# Title with (entry:${A}) is not a link`,
        ``,
        `Real [one](entry:${A}) and [two](entry:${B}) and [one again](entry:${A}).`,
        ``,
        `Code \`[fake](entry:${A})\` stays text.`,
        ``,
        "```",
        `[fenced](entry:${A})`,
        "```",
        ``,
        `![image](entry:${A}) is an image, not a link.`,
        ``,
        `[titled](entry:${B} "Training notes") still counts.`,
        ``,
        `[ref][t] counts too.`,
        ``,
        `[t]: entry:${A}`,
        ``,
        `[broken](entry:${A}extra) is not canonical.`,
    ].join("\n");
    assert.deepEqual(outgoingEntryLinks(src), [A, B]);
});

test("reference definitions and duplicates resolve to one id each", () => {
    const src = `[a][t]\n\n[b][t]\n\n[t]: entry:${B}\n`;
    assert.deepEqual(outgoingEntryLinks(src), [B]);
    assert.deepEqual(outgoingEntryLinks("No links here."), []);
});

test("outgoing refs carry labels and ranges for panel actions", () => {
    const src = `See [Training notes](entry:${A}) and [again](entry:${A}).\n`;
    const refs = outgoingEntryRefs(src);
    assert.equal(refs.length, 2);
    assert.equal(refs[0].id, A);
    assert.equal(refs[0].label, "Training notes");
    assert.ok(refs[0].from < refs[0].to);
    assert.equal(refs[1].label, "again");
    assert.ok(refs[0].to <= refs[1].from);
    // The ranges slice exactly their labels out of the document.
    const doc = parseMarkdown(src);
    for (const ref of refs) {
        assert.equal(doc.textBetween(ref.from, ref.to, "\n"), ref.label);
    }
});

test("refs ignore code, images and invalid destinations", () => {
    const src = `Real [one](entry:${A}).\n\n\`[fake](entry:${A})\`\n\n![img](entry:${A})\n\n[bad](entry:${A}extra)\n`;
    const refs = outgoingEntryRefs(src);
    assert.deepEqual(
        refs.map((r) => r.label),
        ["one"],
    );
});

test("linksToTarget matches case-insensitively on real marks only", () => {
    assert.ok(linksToTarget(`[x](entry:${A})`, A));
    assert.ok(linksToTarget(`[x](entry:${A.toUpperCase()})`, A));
    assert.ok(!linksToTarget(`[x](entry:${B})`, A));
    assert.ok(!linksToTarget(`\`[x](entry:${A})\``, A));
    assert.ok(!linksToTarget(`![x](entry:${A})`, A));
    assert.ok(!linksToTarget(`[x](entry:${A}extra)`, A));
});

test("verifyBacklinkCandidates keeps only genuinely linking sources", () => {
    const cand = (id, body, extra = {}) => ({
        id,
        entry_date: "2026-10-31",
        title: `Source ${id}`,
        folder_id: null,
        body,
        ...extra,
    });
    const candidates = [
        cand("s1", `See [mine](entry:${A}).`),
        cand("s2", `Code \`[mine](entry:${A})\` is not a link.`),
        cand("s3", `![img](entry:${A}) is an image.`),
        cand("s1", `See [mine](entry:${A}) again.`), // same source twice
        cand("s4", `Nothing here.`),
        cand(A, `Self [link](entry:${A}).`), // never shown, even if served
    ];
    assert.deepEqual(
        verifyBacklinkCandidates(A, candidates).verified.map((v) => v.id),
        ["s1"],
    );
    assert.deepEqual(verifyBacklinkCandidates(A, candidates).pending, []);
});

test("verifyBacklinkCandidates preserves order and metadata", () => {
    const candidates = [
        { id: "s2", entry_date: "2026-10-30", title: "Later", folder_id: "f1", body: `[t](entry:${B})` },
        { id: "s1", entry_date: "2026-10-29", title: null, folder_id: null, body: `[t](entry:${B})` },
    ];
    assert.deepEqual(verifyBacklinkCandidates(B, candidates).verified, [
        { id: "s2", entry_date: "2026-10-30", title: "Later", folder_id: "f1" },
        { id: "s1", entry_date: "2026-10-29", title: null, folder_id: null },
    ]);
});

test("verifyBacklinkCandidates parks oversized bodies as pending", () => {
    const big = {
        id: "big",
        entry_date: "2026-10-30",
        title: "Long",
        folder_id: null,
        body: null,
        body_size: 100_000,
        rev: 7,
    };
    const small = {
        id: "s1",
        entry_date: "2026-10-29",
        title: null,
        folder_id: null,
        body: `[t](entry:${B})`,
        body_size: 20,
        rev: 3,
    };
    const out = verifyBacklinkCandidates(B, [big, small]);
    assert.deepEqual(
        out.verified.map((v) => v.id),
        ["s1"],
    );
    // Pending keeps the metadata the on-demand fetch needs; the
    // unverifiable body is never guessed at.
    assert.deepEqual(out.pending, [
        {
            id: "big",
            entry_date: "2026-10-30",
            title: "Long",
            folder_id: null,
            rev: 7,
            body_size: 100_000,
        },
    ]);
});

test("backlink verification caches by source revision", () => {
    clearBacklinkCache();
    const linked = {
        id: "s1",
        entry_date: "2026-10-29",
        title: null,
        folder_id: null,
        body: `[t](entry:${B})`,
        body_size: 20,
        rev: 3,
    };
    assert.deepEqual(
        verifyBacklinkCandidates(B, [linked]).verified.map((v) => v.id),
        ["s1"],
    );
    // Same id and rev, body now link-free: the cache still reports
    // the verified row instead of re-parsing.
    const edited = { ...linked, body: "Nothing here." };
    assert.deepEqual(
        verifyBacklinkCandidates(B, [edited]).verified.map((v) => v.id),
        ["s1"],
    );
    // A new revision re-parses and drops the row.
    const changed = { ...linked, body: "Nothing here.", rev: 4 };
    assert.deepEqual(verifyBacklinkCandidates(B, [changed]).verified, []);
    // Clearing forgets everything.
    clearBacklinkCache();
    assert.deepEqual(verifyBacklinkCandidates(B, [edited]).verified, []);
});
