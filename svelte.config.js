import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

/**
 * Lets a component keep its scoped CSS in a sidecar file. It is inlined
 * before Svelte compiles the component, preserving normal CSS scoping.
 */
const sidecarFiles = {
    async markup({ content, filename }) {
        const componentDir = dirname(filename);
        const block = /<style\s+src=(['"])(\.\.?\/[^'"]+)\1\s*><\/style>/g;
        const replacements = [];
        const dependencies = [];

        for (const match of content.matchAll(block)) {
            const [, , source] = match;
            const path = resolve(componentDir, source);
            const sidecar = await readFile(path, 'utf8');
            dependencies.push(path);
            replacements.push({
                from: match[0],
                to: `<style>\n${sidecar}\n</style>`,
            });
        }

        for (const { from, to } of replacements) content = content.replace(from, to);
        return { code: content, dependencies };
    },
};

export default {
    preprocess: sidecarFiles,
};
