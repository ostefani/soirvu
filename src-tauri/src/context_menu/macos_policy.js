// SPDX-License-Identifier: MIT
(() => {
    Object.defineProperty(window, 'soirvuNativeEditMenu', { value: true });
    const cancel = (event) => event.preventDefault();
    window.addEventListener('contextmenu', cancel, true);
    window.addEventListener('webkitmouseforcewillbegin', cancel, true);
    window.addEventListener('keydown', (event) => {
        if (event.ctrlKey && event.metaKey && event.code === 'KeyD') event.preventDefault();
    }, true);
})();
