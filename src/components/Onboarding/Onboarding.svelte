<script>
    import Button from '../Button/Button.svelte';
    import Input from '../Input/Input.svelte';

    let { model = $bindable(), fields, actions } = $props();
</script>
<style src="./Onboarding.css"></style>

<div class="center-col">
    {#if model.step === 1}
        <p>Create your diary in a folder you choose. Everything in it is encrypted.</p>
        {#each actions.pick as action (action.name)}
            <Button {...action} />
        {/each}
    {:else if model.step === 2}
        <p>Diary folder: <strong>{model.folderName}</strong></p>
        {#if model.warning}<p class="note">{model.warning}</p>{/if}
        {#each fields as field (field.name)}
            <Input {field} bind:value={model[field.name]} />
        {/each}
        {#each actions.create as action (action.name)}
            <Button {...action} />
        {/each}
    {:else}
        <p>
            This is your <strong>recovery key</strong>. If you forget the passphrase, it is the only way back in. The
            app does not keep a copy — write it down and keep it somewhere safe.
        </p>
        <p class="recovery-key">{model.recoveryKey}</p>
        <label class="check">
            <input type="checkbox" bind:checked={model.saved} />
            I have saved the recovery key
        </label>
        {#each actions.done as action (action.name)}
            <Button {...action} />
        {/each}
    {/if}

    {#if model.step < 3}
        {#each actions.cancel as action (action.name)}
            <Button {...action} />
        {/each}
    {/if}

    {#if model.error}<p class="error">{model.error}</p>{/if}
</div>
