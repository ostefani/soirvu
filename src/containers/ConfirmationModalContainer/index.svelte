<!-- SPDX-License-Identifier: MIT
     Copyright (c) 2026 Olha Stefanishyna -->
<script>
    import { onDestroy } from 'svelte';
    import ConfirmationModal from '@/components/ConfirmationModal/index.svelte';
    import { createConfirmation } from '@/lib/confirmation.svelte.js';

    let { onConfirm, onConfirmed, onCancel, describeError, ...content } = $props();
    const confirmation = createConfirmation({
        onConfirm: () => onConfirm(),
        onConfirmed: () => onConfirmed?.(),
        describeError: (failure) => describeError(failure),
    });
    onDestroy(() => confirmation.dispose());
</script>

<ConfirmationModal {...content} busy={confirmation.busy} error={confirmation.error} onConfirm={() => confirmation.confirm()} {onCancel} />
