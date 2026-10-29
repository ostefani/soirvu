// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

(function () {
    "use strict";

    var root = document.documentElement;
    var toggle = document.getElementById("theme-toggle");

    function applyTheme(theme) {
        root.setAttribute("data-theme", theme);
        document.querySelectorAll("img[data-themed]").forEach(function (img) {
            var src = theme === "light" ? img.dataset.light : img.dataset.dark;
            if (src && img.getAttribute("src") !== src) img.setAttribute("src", src);
        });
        toggle.textContent = theme === "light" ? "☾" : "☀";
        toggle.setAttribute("aria-label", theme === "light" ? "Switch to dark theme" : "Switch to light theme");
        try {
            localStorage.setItem("soirvu-theme", theme);
        } catch (e) {
        }
    }

    function initialTheme() {
        try {
            var stored = localStorage.getItem("soirvu-theme");
            if (stored === "light" || stored === "dark") return stored;
        } catch (e) {
        }
        return window.matchMedia("(prefers-color-scheme: light)").matches
            ? "light"
            : "dark";
    }

    applyTheme(initialTheme());

    toggle.addEventListener("click", function () {
        applyTheme(root.getAttribute("data-theme") === "light" ? "dark" : "light");
    });

    var facade = document.getElementById("video-facade");
    var video = document.getElementById("walkthrough");
    facade.addEventListener("click", function () {
        facade.hidden = true;
        video.hidden = false;
        video.play().catch(function () {
        });
    });

    document.getElementById("year").textContent = new Date().getFullYear();
})();
