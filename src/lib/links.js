// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Vault-local link analysis, framework-free. The canonical stored
// form is `[label](entry:<uuid>)` — a normal inline Markdown link
// whose destination carries only a canonical UUID (no paths,
// authority, queries or fragments), analogous to `attachment:`.
// Both the parser gate (markdown.js) and authoring validation use
// this module, so the editor can never display a link the
// verifier would reject. Outgoing links come from the same
// `parseMarkdown` the editor renders, never from regexes over
// source text — so code samples, images and raw blocks cannot
// fake an edge.

import { parseMarkdown, serializeMarkdown } from "../editor/markdown.js";
import { schema } from "../editor/schema.js";
import { parseEntryTarget } from "./entry-target.js";

// Identity and classification live in the dependency-free leaf;
// re-exported here so existing importers keep working while the
// parser gate and editor helpers import the leaf directly.
export { classifyHref, isEntryHref, parseEntryTarget } from "./entry-target.js";

export function entryLinkMarkdown(id, label) {
    const target = parseEntryTarget(`entry:${id}`);
    if (!target || typeof label !== 'string' || !label.trim()) throw new Error('Invalid entry link');
    const mark = schema.marks.link.create({ href: `entry:${target}` });
    const text = schema.text(label.replace(/\s+/g, ' ').trim(), [mark]);
    return serializeMarkdown(schema.nodes.doc.create(null, schema.nodes.paragraph.create(null, text)));
}


export function readEntryLink(text) {
    const source = text.trim();
    const id = parseEntryTarget(source);
    if (id) return { id, label: '' };
    const doc = parseMarkdown(source);
    if (doc.childCount !== 1 || doc.firstChild.type.name !== 'paragraph') return null;
    const refs = analyzeEntryLinks(doc).refs;
    if (refs.length !== 1 || refs[0].from !== 1 || refs[0].to !== doc.firstChild.content.size + 1) return null;
    return { id: refs[0].id, label: refs[0].label };
}


/**
 * One traversal of an already-parsed document producing every
 * view the callers need: refs with labels and exact ranges in
 * document order (adjacent text sharing one link mark merges),
 * ordered unique target ids, and target membership. Images,
 * code and raw blocks are nodes or unmarked text, never link
 * marks, so they contribute nothing by construction; inline
 * code is additionally skipped defensively.
 * @param {import("prosemirror-model").ProseMirrorNode} doc
 * @returns {{ refs: { id: string, label: string, from: number, to: number }[], ids: string[], has: (id: string) => boolean }}
 */
export function analyzeEntryLinks(doc) {
    const refs = [];
    const seen = new Set();
    const ids = [];
    doc.descendants((node, pos) => {
        if (!node.isText) return true;
        if (node.marks.some((m) => m.type.name === "code")) return false;
        const mark = node.marks.find((m) => m.type.name === "link");
        if (!mark) return false;
        const id = parseEntryTarget(mark.attrs.href);
        if (id === null) return false;
        const start = pos;
        const end = pos + node.nodeSize;
        const last = refs[refs.length - 1];
        if (last && last.id === id && last.to === start) {
            last.to = end;
            last.label += node.text ?? "";
        } else {
            refs.push({ id, label: node.text ?? "", from: start, to: end });
        }
        if (!seen.has(id)) {
            seen.add(id);
            ids.push(id);
        }
        return false;
    });
    return {
        refs,
        ids,
        has: (id) => {
            const key = parseEntryTarget(`entry:${id ?? ""}`);
            return key !== null && seen.has(key);
        },
    };
}

/**
 * Entry ids linked from a body, in document order, deduplicated.
 * @param {string} markdown
 * @returns {string[]}
 */
export function outgoingEntryLinks(markdown) {
    return analyzeEntryLinks(parseMarkdown(markdown ?? "")).ids;
}

/**
 * Every entry link in a body with its label and exact document
 * range, in document order. The ranges slice their labels out
 * of the parsed document, so panel actions (navigate, remove)
 * anchor exactly.
 * @param {string} markdown
 * @returns {{ id: string, label: string, from: number, to: number }[]}
 */
export function outgoingEntryRefs(markdown) {
    return analyzeEntryLinks(parseMarkdown(markdown ?? "")).refs;
}

/**
 * Whether a body genuinely links one entry. Only real link
 * marks count — the verifier's whole point is that FTS bodies
 * over-return (code samples, images, near-miss destinations).
 * @param {string} markdown
 * @param {string} targetId
 */
export function linksToTarget(markdown, targetId) {
    const want = parseEntryTarget(`entry:${targetId ?? ""}`);
    if (want === null) return false;
    return analyzeEntryLinks(parseMarkdown(markdown ?? "")).has(want);
}

/**
 * Backlink verification booleans, keyed by target, source and the
 * server-provided content hash — never by revision, which sync
 * can reuse for replaced content. Only the verdict is cached;
 * the displayed row is always built from the current candidate
 * metadata, so a title-only edit (same hash) never serves stale
 * text. A reopened panel skips re-parsing; an oversized source
 * verified once never refetches while its hash still matches.
 * Bounded FIFO: the oldest entries leave first. Cleared on lock
 * via `clearBacklinkCache`.
 * @type {Map<string, boolean>}
 */
const backlinkCache = new Map();
const BACKLINK_CACHE_LIMIT = 200;

/** Forgets every cached verification (lock, vault switch). */
export function clearBacklinkCache() {
    backlinkCache.clear();
}

function backlinkCacheKey(targetId, id, bodyHash) {
    return `${targetId}:${id}:${bodyHash}`;
}

function backlinkCacheGet(targetId, candidate) {
    if (typeof candidate?.body_hash !== "string") return undefined;
    const hit = backlinkCache.get(backlinkCacheKey(targetId, candidate.id, candidate.body_hash));
    return hit === undefined ? undefined : hit;
}

function backlinkCachePut(targetId, candidate, links) {
    if (typeof candidate?.body_hash !== "string") return;
    if (backlinkCache.size >= BACKLINK_CACHE_LIMIT) {
        backlinkCache.delete(backlinkCache.keys().next().value);
    }
    backlinkCache.set(backlinkCacheKey(targetId, candidate.id, candidate.body_hash), links);
}

/** Builds the displayed row from current candidate metadata. */
function backlinkRow(candidate) {
    return {
        id: candidate.id,
        entry_date: candidate.entry_date,
        title: candidate.title ?? null,
        folder_id: candidate.folder_id ?? null,
    };
}

/**
 * True when a slice budget is spent and the scan should yield to
 * the event loop before parsing the next body, so a stop click
 * or a paint lands between documents instead of after the page.
 * @param {number} nowMs
 * @param {number} lastYieldMs
 * @param {number} budgetMs
 */
export function shouldYield(nowMs, lastYieldMs, budgetMs) {
    return nowMs - lastYieldMs >= budgetMs;
}

/**
 * Only a confirmed-missing entry skips silently. Every other
 * fetch failure (lock, storage) aborts the scan with an error so
 * it never reports completion over unread sources.
 * @param {unknown} e IPC rejection value.
 */
export function isMissingEntryError(e) {
    return e === "entry_not_found";
}

/**
 * Splits candidate sources into genuinely linking rows and
 * oversized sources the command sent as metadata only (`body`
 * null): the latter are never guessed at — the caller fetches
 * them whole on demand. In candidate order, one row per source.
 * Self-links never list, even if served. Parsing stays
 * proportional to the inline bodies; cached content hashes skip
 * parsing entirely.
 * @param {string} targetId
 * @param {{ id: string, entry_date: string, title: string | null, folder_id: string | null, body: string | null, body_size?: number, body_hash?: string }[]} candidates
 * @returns {{ verified: { id: string, entry_date: string, title: string | null, folder_id: string | null }[], pending: { id: string, entry_date: string, title: string | null, folder_id: string | null, body_hash: string | null, body_size: number }[] }}
 */
export function verifyBacklinkCandidates(targetId, candidates) {
    const want = parseEntryTarget(`entry:${targetId ?? ""}`);
    if (want === null) return { verified: [], pending: [] };
    const seen = new Set();
    const verified = [];
    const pending = [];
    for (const candidate of candidates ?? []) {
        const id = candidate?.id;
        if (typeof id !== "string" || seen.has(id)) continue;
        seen.add(id);
        if (id.toLowerCase() === want) continue;
        if (candidate.body == null) {
            // Metadata only: a cached verdict under the same
            // content hash serves without refetching; otherwise
            // the caller fetches the body whole and re-runs
            // this function.
            const cached = backlinkCacheGet(want, candidate);
            if (cached !== undefined) {
                if (cached) verified.push(backlinkRow(candidate));
                continue;
            }
            pending.push({
                id,
                entry_date: candidate.entry_date,
                title: candidate.title ?? null,
                folder_id: candidate.folder_id ?? null,
                body_hash: typeof candidate.body_hash === "string" ? candidate.body_hash : null,
                body_size: candidate.body_size ?? 0,
            });
            continue;
        }
        const cached = backlinkCacheGet(want, candidate);
        if (cached !== undefined) {
            if (cached) verified.push(backlinkRow(candidate));
            continue;
        }
        const links = linksToTarget(candidate.body ?? "", want);
        if (links) verified.push(backlinkRow(candidate));
        backlinkCachePut(want, candidate, links);
    }
    return { verified, pending };
}


