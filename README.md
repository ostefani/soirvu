![Tauri](https://img.shields.io/badge/app%20shell-Tauri%202-24C8D8)
![Rust](https://img.shields.io/badge/language-Rust-orange)
![Svelte](https://img.shields.io/badge/UI-Svelte%205-FF3E00)
![JavaScript](https://img.shields.io/badge/language-JavaScript-yellow)
![ProseMirror](https://img.shields.io/badge/editor-ProseMirror-673AB7)

![Vite](https://img.shields.io/badge/build-Vite-646CFF)
![SQLCipher](https://img.shields.io/badge/database-SQLCipher-003B57)
![Argon2id](https://img.shields.io/badge/KDF-Argon2id-4B275F)
![XChaCha20-Poly1305](https://img.shields.io/badge/AEAD-XChaCha20--Poly1305-darkgreen)
![Markdown](https://img.shields.io/badge/format-Markdown-000000)
![macOS](https://img.shields.io/badge/platform-macOS-lightgrey)

![CLI](https://img.shields.io/badge/type-Desktop%20App-informational)
![Interface](https://img.shields.io/badge/interface-GUI-informational)
![Visitors](https://visitor-badge.laobi.icu/badge?page_id=ostefani.soirvu)
<!-- ![License](https://img.shields.io/badge/license-MIT-green) -->

# soirvu

*soir* + *vu* — "the evening, seen." A local-first, encrypted personal diary.

soirvu keeps your diary in an encrypted vault on your own machine. There is no account, no cloud and no network access: your entries are unreadable to anything that reads the files, and readable only by you, with your passphrase.

## Features

- [ ] **Encrypted vault** — entries live in a SQLCipher database; attachments are stored as separately encrypted files. Keys are derived with Argon2id and wrapped in LUKS-style key slots, so changing your passphrase doesn't re-encrypt your diary.
- [ ] **Recovery key** — a randomly generated key shown once at setup lets you back in if the passphrase is forgotten. Without either, the diary is unrecoverable — by design.
- [ ] **Markdown, always** — entries are stored as plain Markdown and edited in a WYSIWYG editor built on ProseMirror. No proprietary format: export everything to a folder of `.md` files in one click, or use the `diary-cli` tool to verify, export or rekey a vault without the app.
- [ ] **Your choice of location** — the vault is an ordinary folder: internal disk, external drive, or a mounted encrypted disk image.
- [ ] **Encrypted full-text search** — search runs inside the encrypted database; nothing is indexed in plaintext.
- [ ] **Auto-lock** — the vault locks on idle, sleep, screen lock or manual shortcut, and the keys are wiped from memory.

## Security model

The Rust core owns all keys, all file access and all cryptography; the UI never sees a key or a filesystem path. Encryption protects your diary at rest — it cannot protect text while you are reading it, so auto-lock exists to shrink that window. Keyloggers, screen recorders and a compromised OS are explicitly out of scope.

## Tech

Tauri v2 · Rust core (SQLCipher, XChaCha20-Poly1305, Argon2id) · Svelte 5 UI · ProseMirror editor with Markdown as the only stored format.

## Status

In design / early development. macOS first; Linux and Windows later.

## Building

Prerequisites: Rust toolchain and Node.js.

```sh
npm install
npm run tauri dev

# run leak test
sh scripts/leak-test.sh <path-to-test_diar-folder>
# run test
INSTA_UPDATE=always cargo test -p soirvu --lib
```

## License

Copyright (c) 2026 Olha Stefanishyna
All rights reserved.
