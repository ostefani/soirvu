# Building SoirVu

SoirVu's release installers are **unsigned**. macOS shows the
usual unidentified-developer prompt, Windows shows a
SmartScreen warning, Ubuntu shows nothing special. That is a
deliberate trade: the trust anchor is not a certificate, it
is this document. Anyone can rebuild the app from a tagged
commit and compare, and each release publishes `SHA256SUMS`
with a minisign signature:

```
minisign -Vm SHA256SUMS -P RWR7gXjtSq1lhJBJCuEa6CSPBBwsUuvK08vQ7K6OhR5ljSNCpymcNXGX
sha256sum -c SHA256SUMS
```

## Common prerequisites

- Rust (stable, via <https://rustup.rs>)
- Node.js 24 LTS and npm
- git

Then, in the repository root: `npm ci --ignore-scripts`.

## macOS

Additional prerequisite: the Xcode command-line tools
(`xcode-select --install`).

```
npm run tauri build
```

produces the app and a `.dmg` under
`target/release/bundle/`. The build is ad-hoc
signed with the hardened runtime and no entitlements;
`scripts/verify-release.sh` checks a built app.

### The Touch ID variant (macOS only)

Touch ID is an opt-in cargo feature and needs a development
profile with the keychain entitlement — a free Personal Team
profile expires after about a week, which is why the normal
build excludes it. See `src-tauri/tauri.touchid.conf.json`;
in short: copy a current profile for this bundle id to
`src-tauri/touchid.provisionprofile` (gitignored), then

```
APPLE_SIGNING_IDENTITY="<identity hash from security find-identity -v -p codesigning>" \
  npm run tauri build -- --config src-tauri/tauri.touchid.conf.json --features touch-id
```

## Ubuntu

Supported target: the current Ubuntu LTS, on X11 and
Wayland. Other distributions are not supported, but the
source build below is the same shape.

System packages (Tauri's WebKitGTK stack, plus the
GStreamer plugins the webview plays media with):

```
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad
```

```
npm run tauri build
```

produces a `.deb` under `target/release/bundle/deb/`.
Install with `sudo apt install ./<the file>.deb`.

## Windows

Windows is CI-supported: every release is built, tested, and
install/uninstall smoke-tested on Windows in CI. The maintainer
has no Windows machine, so editor and lock behaviour there is
verified by user reports rather than daily use.
Prerequisites:

- Visual Studio Build Tools with the C++ workload (MSVC)
- Strawberry Perl (the vendored OpenSSL build needs it)
- NASM, if the OpenSSL build asks for an assembler

```
npm run tauri build
```

produces a per-user NSIS installer under
`target/release/bundle/nsis/` — it needs no admin
rights and never touches existing vaults, on install,
upgrade or uninstall.

## Development

`npm run tauri dev` runs the app with hot reload. Dev
builds keep their registry, settings and keychain items
separate from release builds ("SoirVu Dev" app data,
`soirvu.device.dev` keychain service), so developing never
touches real diaries' bookkeeping.

## Releases

Releases are cut from a tag: push `vX.Y.Z` and the release
workflow builds the installers on all three OSes, runs the
per-OS release verification and an install/uninstall smoke
test on each, and publishes a GitHub release with
`SHA256SUMS` and its minisign signature (`SHA256SUMS.minisig`).
Uninstalling never touches vault folders, on any OS — the
smoke tests assert exactly that.

The installers are unsigned (except the macOS ad-hoc
signature); the checksums plus the signature are how a
download is verified, and building from source remains
the trust anchor. Releases are immutable (repo setting):
once published, the tag and all assets are locked, so a
published version can never be re-pointed or re-uploaded —
fixes go out as a new version. The Touch ID macOS variant
is not part of the release workflow — it needs the local
provisioning profile, so it is built by hand (see above).

One-time setup: create the signing keypair and add the
secret half to the repository's Actions secrets
(`MINISIGN_SECRET_KEY`, and `MINISIGN_PASSWORD` if the key
has one). Put the public key here and in the README:

```
minisign -G -p soirvu.pub -s soirvu.key
# public key: RWR7gXjtSq1lhJBJCuEa6CSPBBwsUuvK08vQ7K6OhR5ljSNCpymcNXGX
```

Verifying a downloaded release:

```
sha256sum -c SHA256SUMS
minisign -Vm SHA256SUMS -p soirvu.pub
```
