// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Vault-local link identity and URL classification. This is a leaf:
// no Markdown parser, Svelte or IPC dependency, so the parser gate
// (editor/markdown.js) can use it without forming an import cycle
// with the analysis module (lib/links.js) that consumes the gate.

const UUID = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";
const ENTRY_HREF = new RegExp(`^entry:(${UUID})$`);
const MAX_OUTBOUND_URL_BYTES = 4096;
const UNSAFE_URL_TEXT = /[\p{Cc}\p{Cf}]/u;
const urlEncoder = new TextEncoder();

/** Bounds link attributes before they reach the editor DOM or native dialog. */
export function isWithinLinkLimit(value) {
    return value.length <= MAX_OUTBOUND_URL_BYTES && urlEncoder.encode(value).length <= MAX_OUTBOUND_URL_BYTES;
}

/** Normalizes an HTTPS destination using the native opener's size and authority limits. */
export function normalizeWebHref(href) {
    if (!/^https:\/\/[^/?#\s<>\\@]+(?:[/?#]|$)/i.test(href)
        || /[\s<>\\]/u.test(href) || !isWithinLinkLimit(href)) return null;
    try {
        // Match Rust's percent decoder: a stray percent stays literal.
        const escaped = href.replace(/%(?![\da-f]{2})/gi, "%25");
        // Reject all format characters, including ZWJ and ZWNJ, in outbound URLs.
        if (UNSAFE_URL_TEXT.test(href) || UNSAFE_URL_TEXT.test(decodeURIComponent(escaped))) return null;
        const url = new URL(href);
        return url.hostname && isWithinLinkLimit(url.href) ? url.href : null;
    } catch {
        return null;
    }
}

/**
 * The target entry id (lowercased) when `href` is a canonical
 * vault-local link, else null. Comparison is case-insensitive
 * because imports may spell hex either way; app-generated links
 * are always literal lowercase.
 * @param {unknown} href
 * @returns {string | null}
 */
export function parseEntryTarget(href) {
    if (typeof href !== "string") return null;
    const m = ENTRY_HREF.exec(href);
    return m ? m[1].toLowerCase() : null;
}

/** Vault-local href predicate for the parser gate. */
export function isEntryHref(href) {
    return parseEntryTarget(href) !== null;
}

/**
 * Never broadens external permissions: only http(s)/mailto
 * leave the diary, only `attachment:` reaches the asset
 * protocol, only canonical `entry:` routes in-vault.
 * @param {unknown} href
 * @returns {"entry" | "external" | "attachment" | "other"}
 */
export function classifyHref(href) {
    if (typeof href !== "string" || href === "") return "other";
    if (isEntryHref(href)) return "entry";
    if (/^(https?:\/\/|mailto:)/i.test(href)) return "external";
    if (/^attachment:/i.test(href)) return "attachment";
    return "other";
}
