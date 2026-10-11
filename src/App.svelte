<script>
    import { invoke } from '@tauri-apps/api/core';

    let name = $state('');
    let greetMsg = $state('');

    /** @param {SubmitEvent} event */
    async function greet(event) {
        event.preventDefault();
        if (!('__TAURI_INTERNALS__' in window)) {
            greetMsg = 'Not inside Tauri — run: npm run tauri dev';
            return;
        }

        // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
        greetMsg = await invoke('greet', { name });
        name = '';
    }
</script>

<main class="container">
    <h1>soirvu</h1>

    <form class="row" onsubmit={greet}>
        <input bind:value={name} placeholder="Enter a name..." />
        <button type="submit">Greet</button>
    </form>
    <p>{greetMsg}</p>
</main>

<style>
    .container {
        margin: 0;
        padding-top: 10vh;
        display: flex;
        flex-direction: column;
        justify-content: center;
        text-align: center;
    }

    .row {
        display: flex;
        justify-content: center;
        gap: 0.5em;
    }
</style>
