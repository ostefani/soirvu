export const TOOLBAR_TEXT = {
    toolbarLabel: 'Formatting',
    attach: 'Attach…',
    linkEntry: 'Link…',
    linkEntryTitle: 'Link selected text to an entry',
    languagePlaceholder: 'language',
};

export const TOOLBAR_FORMAT_COMMANDS = [
    { id: 'strong', text: 'B', title: 'Bold (Ctrl+B)', textClass: 'tb-bold', buttonClass: '', active: selectedFlag('strong') },
    { id: 'em', text: 'I', title: 'Italic (Ctrl+I)', textClass: 'tb-italic', buttonClass: '', active: selectedFlag('em') },
    { id: 'strike', text: 'S', title: 'Strikethrough (Ctrl+Shift+X)', textClass: '', buttonClass: 'tb-strike', active: selectedFlag('strike') },
    { id: 'code', text: '</>', title: 'Inline code (Ctrl+E)', textClass: '', buttonClass: '', active: selectedFlag('code') },
];

export const TOOLBAR_TYPE_COMMANDS = [
    { id: 'paragraph', text: '¶', title: 'Paragraph', textClass: '', buttonClass: '', active: (selection) => selection?.block === 'paragraph' && !selection?.list },
    { id: 'heading1', text: 'H1', title: 'Heading 1', textClass: '', buttonClass: '', active: (selection) => selection?.headingLevel === 1 },
    { id: 'heading2', text: 'H2', title: 'Heading 2', textClass: '', buttonClass: '', active: (selection) => selection?.headingLevel === 2 },
    { id: 'heading3', text: 'H3', title: 'Heading 3', textClass: '', buttonClass: '', active: (selection) => selection?.headingLevel === 3 },
];

export const TOOLBAR_LIST_COMMANDS = [
    { id: 'quote', text: '❝', title: 'Quote', textClass: '', buttonClass: '', active: selectedFlag('quote') },
    { id: 'bulletList', text: '• List', title: 'Bullet list', textClass: '', buttonClass: '', active: (selection) => selection?.list === 'bullet' },
    { id: 'orderedList', text: '1. List', title: 'Numbered list', textClass: '', buttonClass: '', active: (selection) => selection?.list === 'ordered' },
    { id: 'taskList', text: '☑ Task', title: 'Task list', textClass: '', buttonClass: '', active: (selection) => selection?.list === 'task' },
];

export const TOOLBAR_TABLE_COMMANDS = [
    { id: 'insertTable', text: 'Table', title: 'Insert table', textClass: '', buttonClass: '', inTableOnly: false, active: () => false },
    { id: 'addRow', text: '+Row', title: 'Add a row below', textClass: '', buttonClass: '', inTableOnly: true, active: () => false },
    { id: 'addColumn', text: '+Col', title: 'Add a column to the right', textClass: '', buttonClass: '', inTableOnly: true, active: () => false },
    { id: 'deleteRow', text: '−Row', title: 'Delete this row', textClass: '', buttonClass: '', inTableOnly: true, active: () => false },
    { id: 'deleteColumn', text: '−Col', title: 'Delete this column', textClass: '', buttonClass: '', inTableOnly: true, active: () => false },
];

export const TOOLBAR_CODE_BLOCK = { id: 'codeBlock', text: '{ }', title: 'Code block' };

function selectedFlag(name) {
    return (selection) => !!selection?.[name];
}
