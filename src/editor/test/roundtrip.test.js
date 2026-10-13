// Spike 9, executable form (DESIGN §13, Phase 0): does a note
// survive parse -> serialize with its meaning intact, does the
// second round trip change nothing (normalized form is stable),
// and do unsupported blocks survive byte for byte as raw_block?

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { parseMarkdown, serializeMarkdown } from "../src/index.js";

const corpus = (name) =>
    readFileSync(
        fileURLToPath(new URL(`corpus/${name}`, import.meta.url)),
        "utf8",
    ).replace(/\n$/, "");

const ROUND_TRIP = [
    "basic",
    "lists",
    "code",
    "media",
    "intl",
    "raw",
    "hostile",
    "messy",
];

for (const name of ROUND_TRIP) {
    test(`corpus ${name}: first pass matches the reviewed canonical form`, () => {
        assert.equal(serializeMarkdown(parseMarkdown(corpus(`${name}.md`))), corpus(`${name}.expected.md`));
    });
    test(`corpus ${name}: canonical form is a fixed point`, () => {
        const expected = corpus(`${name}.expected.md`);
        assert.equal(serializeMarkdown(parseMarkdown(expected)), expected);
    });
}

test("the table in raw.md is a raw_block, holding its exact lines", () => {
    const doc = parseMarkdown(corpus("raw.md"));
    const raws = [];
    doc.descendants((node) => {
        if (node.type.name === "raw_block") raws.push(node.textContent);
    });
    assert.equal(raws.length, 3); // table, HTML block, footnote def
    assert.ok(raws[0].startsWith("| Day | Mood | Note |"));
    assert.ok(raws[1].startsWith('<div class="embed">'));
    assert.ok(raws[2].startsWith("[^1]: The footnote text"));
});

test("a deleted-entry danger never happens: footnote def with no recognized ref is kept", () => {
    const src = "Text only.\n\n[^9]: Orphan definition.";
    assert.equal(serializeMarkdown(parseMarkdown(src)), src);
});

test("a table inside a blockquote degrades to one raw block, byte exact", () => {
    const src = "> quote\n>\n> | A |\n> |---|\n> | 1 |";
    const doc = parseMarkdown(src);
    assert.equal(doc.childCount, 1);
    assert.equal(doc.firstChild.type.name, "raw_block");
    assert.equal(serializeMarkdown(doc), src);
});

test("CRLF input normalizes to LF and then is stable", () => {
    const out = serializeMarkdown(parseMarkdown("# T\r\n\r\n| A |\r\n|---|\r\n| 1 |"));
    assert.ok(!out.includes("\r"));
    assert.equal(serializeMarkdown(parseMarkdown(out)), out);
});

test("link policy: only https, http, mailto and attachment keep their mark", () => {
    const doc = parseMarkdown(
        "[a](https://example.com) [b](mailto:x@example.com) " +
            "[c](ftp://example.com/f) [d](data:text/html;base64,AAAA) [e](javascript:alert)",
    );
    const hrefs = [];
    doc.descendants((node) => {
        for (const mark of node.marks) {
            if (mark.type.name === "link") hrefs.push(mark.attrs.href);
        }
    });
    assert.deepEqual(hrefs, ["https://example.com", "mailto:x@example.com"]);
    // Nothing is lost, but the two rejection layers look different:
    // the ftp link was tokenized and then stripped by the policy, so
    // only its text remains; javascript: and data: were refused by
    // markdown-it itself, so their source stays as literal text.
    assert.ok(doc.textContent.startsWith("a b c "));
    assert.ok(doc.textContent.includes("[d](data:text/html;base64,AAAA)"));
    assert.ok(doc.textContent.includes("[e](javascript:alert)"));
});

test("a block attachment is a node; an inline attachment link stays a link", () => {
    const doc = parseMarkdown(corpus("media.md"));
    const kinds = [];
    doc.descendants((node) => {
        if (node.type.name === "attachment") kinds.push(`attachment:${node.attrs.attachmentId}`);
        if (node.type.name === "image") kinds.push(`image:${node.attrs.src}`);
    });
    assert.deepEqual(kinds, [
        "image:attachment:3f2504e0-4f89-11d3-9a0c-0305e82c3301",
        "attachment:9b2f2072-8b5d-4c1f-a3e0-5d6b7a8c9d0e",
    ]);
});

test("task items carry checked state through the round trip", () => {
    const doc = parseMarkdown("- [ ] todo\n- [x] done\n- plain");
    const states = [];
    doc.descendants((node) => {
        if (node.type.name === "list_item") states.push(node.attrs.checked);
    });
    assert.deepEqual(states, [false, true, null]);
    assert.equal(
        serializeMarkdown(doc),
        "* [ ] todo\n* [x] done\n* plain",
    );
});

test("hostile input produces no html and no script anywhere in the doc", () => {
    const doc = parseMarkdown(corpus("hostile.md"));
    doc.descendants((node) => {
        assert.notEqual(node.type.name, "html");
        for (const mark of node.marks) {
            assert.ok(!/javascript:/i.test(mark.attrs.href || ""));
        }
    });
    // The script block survived — as inert raw text.
    assert.match(serializeMarkdown(doc), /<script>\nalert\("block"\);\n<\/script>/);
});

test("empty input parses to an empty doc and back", () => {
    assert.equal(serializeMarkdown(parseMarkdown("")), "");
});
