export function parseTags(text) {
    return text
        .split(',')
        .map((tag) => tag.trim())
        .filter((tag) => tag !== '');
}

export function normalizeTags(tags) {
    return parseTags(tags.join(', '));
}

export function mergeTags(current, incoming) {
    const known = new Set(current.map((tag) => tag.toLowerCase()));
    const fresh = [];
    for (const tag of incoming) {
        const key = tag.toLowerCase();
        if (known.has(key)) continue;
        known.add(key);
        fresh.push(tag);
    }
    return fresh.length > 0 ? [...current, ...fresh] : current;
}
