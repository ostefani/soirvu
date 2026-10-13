# Golden fixture v1

A real vault, written by soirvu-core at header format v1,
database schema v3. `tests/golden.rs` opens it — with the
passphrase and with the recovery key — and checks the entries,
the tombstone, the search, and the attachment.

Rules:

- Never edit these files. If a change makes the test fail, the
  change broke old vaults: fix the code.
- A deliberate format change means a NEW folder (golden-v2)
  next to this one. This one keeps testing v1 forever.
- `manifest.json` holds the expected content — and the secrets.
  They are public on purpose, for tests only. Never use this
  passphrase for a real vault.
- The `.gitattributes` in this folder marks every file `binary`.
  Keep it: without it, git can mistake a blob for text and carry
  its raw bytes through patches and checkouts as text, which
  changes them.

Made by:

```sh
cargo run -p soirvu-core --example make_golden -- /tmp/golden-v1
```
