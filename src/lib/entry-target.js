// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Vault-local link identity and URL classification. This is a leaf:
// no Markdown parser, Svelte or IPC dependency, so the parser gate
// (editor/markdown.js) can use it without forming an import cycle
// with the analysis module (lib/links.js) that consumes the gate.

const UUID = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";
const ENTRY_HREF = new RegExp(`^entry:(${UUID})$`);

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
