// SPDX-License-Identifier: MIT
(() => {
    // Keep the recognizer installed: without it WebKit can fall back to
    // native dictionary lookup without dispatching the force event.
    const cancel = (event) => event.preventDefault();
    window.addEventListener('contextmenu', cancel, true);
    window.addEventListener('webkitmouseforcewillbegin', cancel, true);
})();
