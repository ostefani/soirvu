<script>
    import { untrack } from 'svelte';
    import { createEditor } from '@/editor/index.js';
    import { assetIdFromSrc, renderAttachment } from './attachments.js';

    let {
        initialMarkdown = '',
        restore = null,
        readonly = false,
        inert = false,
        attachments,
        onReady,
        onChange,
        onSelectionChange,
        onAttachFile,
        onOpenLink,
        onRequestLinkPicker,
    } = $props();

    let editor = $state.raw(null);

    $effect(() => {
        editor?.setReadonly?.(readonly);
    });

    function mount(node) {
        return untrack(() => {
            let instance = null;
            instance = createEditor(node, {
                markdown: initialMarkdown,
                onChange: (markdown) => onChange?.(markdown),
                onSelectionChange: () => onSelectionChange?.(instance?.query() ?? null),
                onAttach: (file) => onAttachFile?.(file),
                onAttachmentView: (el, id) => renderAttachment(el, id, attachments),
                onOpenLink: (href, modified) => onOpenLink?.(href, modified),
                onRequestLinkPicker: ({ bracketPos }) => onRequestLinkPicker?.(bracketPos),
            });
            editor = instance;
            restoreSelection(instance, restore);
            const release = onReady(instance);
            onSelectionChange(instance.query());
            node.addEventListener('dblclick', openAssetImage);

            return () => {
                node.removeEventListener('dblclick', openAssetImage);
                release?.();
                instance.destroy();
                editor = null;
            };
        });
    }

    function restoreSelection(instance, target) {
        if (!target?.sel) return;
        try {
            instance.setTextSelection(target.sel.from, target.sel.to);
        } catch {}
    }

    function openAssetImage(event) {
        const src = event.target.closest('img')?.getAttribute('src') || '';
        const id = assetIdFromSrc(src);
        if (id !== null) attachments.open(id);
    }
</script>

<div class="editor-mount" {inert} {@attach mount}></div>

<style src="./styles.css"></style>
