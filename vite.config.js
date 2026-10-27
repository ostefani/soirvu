import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const host = process.env.TAURI_DEV_HOST;
const __dirname = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
    plugins: [svelte()],
    resolve: {
        alias: {
            // "@/components/Editor.svelte" → "<project>/src/components/Editor.svelte"
            // Safe next to scoped npm packages: only "@/…" is rewritten,
            // "@tauri-apps/…" does not start with "@/" and is left alone.
            '@': resolve(__dirname, 'src'),
        },
    },
    // prevent vite from obscuring rust errors
    clearScreen: false,
    server: {
        // make sure this port matches the devUrl port in tauri.conf.json file
        port: 5173,
        // Tauri expects a fixed port, fail if that port is not available
        strictPort: true,
        // if the host Tauri is expecting is set, use it
        host: host || false,
        hmr: host
            ? {
                protocol: 'ws',
                host,
                port: 1421,
            }
            : undefined,

        watch: {
            // tell vite to ignore watching `src-tauri`
            ignored: ['**/src-tauri/**'],
        },
    },
    // Env variables starting with the item of `envPrefix` will be exposed in tauri's source code through `import.meta.env`.
    envPrefix: ['VITE_', 'TAURI_ENV_*'],
    build: {
        // Tauri uses Chromium on Windows and WebKit on macOS and Linux
        target:
            process.env.TAURI_ENV_PLATFORM == 'windows'
                ? 'chrome105'
                : 'safari13',
        // don't minify for debug builds
        minify: !process.env.TAURI_ENV_DEBUG,
        // produce sourcemaps for debug builds
        sourcemap: !!process.env.TAURI_ENV_DEBUG,
        assetsInlineLimit: (file) => (file.endsWith('.svg') ? false : undefined)
    },
});
