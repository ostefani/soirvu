# Contributing to soirvu

Thanks for your interest. soirvu is a local-first encrypted diary; the bar for
changes is that no behaviour, no data safety, and no threat-model promise may
regress silently.

## How to contribute

- Outside contributors: **fork** the repository, work in a branch, and open a
  pull request against `main`.
- Maintainers work in branches on this repository and also merge via pull
  request — nothing lands on `main` directly.

Branch names use a prefix: `feature/...`, `fix/...`, `doc/...`, `refactor/...`.

## Before you open the PR

Run the same gates CI runs, for the area you touched:

```sh
npm ci --ignore-scripts
npm run check        # Svelte / {@html} hygiene
npm test             # frontend unit tests
npm run build        # must bundle with no external hostnames

cargo test -p soirvu-core -p soirvu-cli
cargo clippy -p soirvu-core -p soirvu-cli --all-targets
cargo check --manifest-path src-tauri/Cargo.toml
```

Rust changes should also be `cargo fmt`-clean. If your change alters vault,
backup, or crypto behaviour, add or update the focused test in the crate's
normal test layout — a bug fix without a regression test will be asked for
one.

## Ground rules

- Preserve file paths, user-visible behaviour, and the uppercase marker
  tokens (`§`, `NOTE`, etc.) the code and docs use as cross-references.
- Never weaken an existing test to make a change pass; if a test asserts
  behaviour your change intentionally alters, say so in the PR.
- Do not commit secrets, key material, or real vaults. Test vaults live in
  `temp/` and stay out of the tree.

## Security issues

Do not file public issues for vulnerabilities. Contact the maintainer
privately (via the profile on the repository owner's GitHub account) so a
fix can land before disclosure.

## License

SoirVu is MIT-licensed. By contributing you agree that your contributions
are released under the same [LICENSE](LICENSE). Every source file carries
an `SPDX-License-Identifier: MIT` header plus its author's copyright line —
new files need both. Include your name or nickname in
[CONTRIBUTORS](CONTRIBUTORS) in the same pull request.
