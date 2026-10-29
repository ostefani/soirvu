// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// Syntax highlighting (Phase 7, §6.5) — view-only tokenizers.
//
// The document never changes: code text stays one plain text
// node in the model, and the view splits its DOM rendering
// into classified spans. A small hand scanner per language
// family, deliberately conservative: when in doubt a token
// stays plain, because a wrong color is worse than none.
// JavaScript regular literals are not recognized (a / is a
// division until proven otherwise), which is the standard
// trade for a scanner this size.

const JS_KEYWORDS = new Set(
    ("const let var function return if else for while do switch case break continue " +
        "new delete typeof instanceof in of class extends super this null undefined true false " +
        "try catch finally throw async await yield import from export default void").split(" "),
);
const RUST_KEYWORDS = new Set(
    ("fn let mut const static ref move type struct enum trait impl for in loop while " +
        "if else match return break continue pub crate mod use where as dyn box self Self " +
        "true false unsafe extern async await").split(" "),
);
const PY_KEYWORDS = new Set(
    ("def class return if elif else for while break continue pass import from as with " +
        "try except finally raise lambda and or not in is None True False async await " +
        "yield global nonlocal del assert print").split(" "),
);
const SH_KEYWORDS = new Set(
    ("if then else elif fi for while until do done case esac function in select " +
        "echo export local readonly set unset shift exit return break continue").split(" "),
);

const CONFIGS = {
    javascript: {
        keywords: JS_KEYWORDS,
        lineComment: "//",
        blockComment: ["/*", "*/"],
        quotes: ['"', "'", "`"],
    },
    rust: {
        keywords: RUST_KEYWORDS,
        lineComment: "//",
        blockComment: ["/*", "*/"],
        quotes: ['"', "'"],
    },
    python: {
        keywords: PY_KEYWORDS,
        lineComment: "#",
        blockComment: null,
        quotes: ['"', "'"],
        tripleQuotes: true,
    },
    bash: {
        keywords: SH_KEYWORDS,
        lineComment: "#",
        blockComment: null,
        quotes: ['"', "'"],
    },
};

const ALIASES = {
    js: "javascript",
    jsx: "javascript",
    ts: "javascript",
    tsx: "javascript",
    mjs: "javascript",
    rs: "rust",
    py: "python",
    python3: "python",
    sh: "bash",
    zsh: "bash",
    shell: "bash",
};

/** The language a code block's info string names, or null. */
export function highlightLanguage(params) {
    const word = (params || "").trim().split(/\s+/)[0].toLowerCase();
    const lang = ALIASES[word] || word;
    return CONFIGS[lang] ? lang : null;
}

const NUMBER = /^(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)/;
const WORD = /^[A-Za-z_$][A-Za-z0-9_$]*/;
const RUST_LIFETIME = /^'[a-z_][A-Za-z0-9_]*/;

/**
 * Splits `code` into classified spans: [{start, end, cls}]
 * with cls one of "kw", "str", "com", "num", "fn". Gaps are
 * plain text. Returns null for an unknown language.
 */
export function tokenize(code, params) {
    const lang = highlightLanguage(params);
    if (!lang) return null;
    const cfg = CONFIGS[lang];
    const spans = [];
    let i = 0;
    const push = (start, end, cls) => {
        if (end > start) spans.push({ start, end, cls });
    };
    while (i < code.length) {
        const rest = code.slice(i);
        // Comments first: a quote inside a comment is not a string.
        if (cfg.lineComment && rest.startsWith(cfg.lineComment)) {
            let end = code.indexOf("\n", i);
            if (end === -1) end = code.length;
            push(i, end, "com");
            i = end;
            continue;
        }
        if (cfg.blockComment && rest.startsWith(cfg.blockComment[0])) {
            let end = code.indexOf(cfg.blockComment[1], i + 2);
            end = end === -1 ? code.length : end + 2;
            push(i, end, "com");
            i = end;
            continue;
        }
        // Strings, with escapes; a Rust lifetime is not a string.
        const ch = code[i];
        if (cfg.quotes.includes(ch)) {
            if (lang === "rust" && ch === "'") {
                // A char literal ('a', '\\n') is a string; a
                // lifetime ('a) is plain text.
                const chr = /^'(\\.|[^\\'])'/.exec(rest);
                if (chr) {
                    push(i, i + chr[0].length, "str");
                    i += chr[0].length;
                    continue;
                }
                const life = RUST_LIFETIME.exec(rest);
                if (life) {
                    i += life[0].length;
                    continue;
                }
            }
            const triple = cfg.tripleQuotes && (rest.startsWith('"""') || rest.startsWith("'''"));
            let j = i + (triple ? 3 : 1);
            const closer = triple ? rest.slice(0, 3) : ch;
            while (j < code.length) {
                if (code[j] === "\\") {
                    j += 2;
                    continue;
                }
                if (code.startsWith(closer, j)) {
                    j += closer.length;
                    break;
                }
                if (!triple && code[j] === "\n") break; // unterminated
                j++;
            }
            push(i, Math.min(j, code.length), "str");
            i = Math.min(j, code.length);
            continue;
        }
        // Numbers.
        const num = NUMBER.exec(rest);
        if (num && (i === 0 || !/[A-Za-z0-9_$]/.test(code[i - 1]))) {
            push(i, i + num[0].length, "num");
            i += num[0].length;
            continue;
        }
        // Words: keyword, call, or plain.
        const word = WORD.exec(rest);
        if (word) {
            const text = word[0];
            if (cfg.keywords.has(text)) {
                push(i, i + text.length, "kw");
            } else if (code[i + text.length] === "(") {
                push(i, i + text.length, "fn");
            }
            i += text.length;
            continue;
        }
        i++;
    }
    return spans;
}
