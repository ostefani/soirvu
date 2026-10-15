//! Low-level crypto: random bytes, Argon2id, key wrapping, sub-key derivation.
//! Nothing here knows about vaults or files.

use std::time::{Duration, Instant};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use data_encoding::BASE32_NOPAD;
use hkdf::Hkdf;
use rand::TryRng;
use rand::rngs::SysRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::error::{Error, Result};

pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
/// 32 bytes of key + 16 bytes of authentication tag.
pub const WRAPPED_LEN: usize = KEY_LEN + 16;

/// 32 secret bytes that are overwritten with zeros when dropped.
pub type Key = Zeroizing<[u8; KEY_LEN]>;

/// Labels that turn the master key into independent sub-keys (HKDF "info").
const LABEL_DB: &str = "soirvu/v1/db";
const LABEL_ATTACHMENTS: &str = "soirvu/v1/attachments";
const LABEL_RECOVERY_KEK: &str = "soirvu/v1/kek/recovery";
const LABEL_DEVICE_KEK: &str = "soirvu/v1/kek/device";

// ---------------------------------------------------------------------------
// Limits for Argon2id settings read from a header.
//
// A header is attacker-controlled data until it has been authenticated, so we
// refuse settings that are too weak (downgrade attack) or absurdly large
// (a "unlock needs 4 TB of RAM" denial of service).
//
// In test builds the lower limits are tiny so tests run in milliseconds.
// ---------------------------------------------------------------------------
#[cfg(not(test))]
const MIN_M_KIB: u32 = 64 * 1024; // 64 MiB
#[cfg(test)]
const MIN_M_KIB: u32 = 8;
#[cfg(not(test))]
const MIN_T: u32 = 3;
#[cfg(test)]
const MIN_T: u32 = 1;
const MAX_M_KIB: u32 = 4 * 1024 * 1024; // 4 GiB
const MAX_T: u32 = 20;
const MAX_P: u32 = 16;

/// Argon2id settings. They are stored in the vault header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Number of passes.
    pub t: u32,
    /// Lanes (parallelism).
    pub p: u32,
}

impl KdfParams {
    /// Rejects settings outside the allowed range.
    pub fn check(&self) -> Result<()> {
        if self.m_kib < MIN_M_KIB || self.t < MIN_T || self.p < 1 {
            return Err(Error::BadKdfParams(format!(
                "too weak: m={} KiB, t={}, p={}",
                self.m_kib, self.t, self.p
            )));
        }
        if self.m_kib > MAX_M_KIB || self.t > MAX_T || self.p > MAX_P {
            return Err(Error::BadKdfParams(format!(
                "too large: m={} KiB, t={}, p={}",
                self.m_kib, self.t, self.p
            )));
        }
        Ok(())
    }

    /// Finds settings that make one unlock take about `target` on THIS machine:
    /// starting at 64 MiB, double the memory until one run takes long enough
    /// (never more than 1 GiB).
    pub fn calibrate(target: Duration) -> Result<KdfParams> {
        let mut chosen = KdfParams { m_kib: 64 * 1024, t: 3, p: 4 };
        for step in 0..=4 {
            let candidate = KdfParams { m_kib: (64 * 1024) << step, t: 3, p: 4 };
            let started = Instant::now();
            derive_kek("calibration", &[0u8; SALT_LEN], &candidate)?;
            chosen = candidate;
            if started.elapsed() >= target {
                break;
            }
        }
        Ok(chosen)
    }
}

/// Fills a fixed-size array with random bytes from the operating system.
pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    SysRng
        .try_fill_bytes(&mut out)
        .map_err(|e| Error::Random(e.to_string()))?;
    Ok(out)
}

fn random_key() -> Result<Key> {
    Ok(Zeroizing::new(random_bytes::<KEY_LEN>()?))
}

/// Unicode text can be written in two equivalent ways (for example «й» as one
/// character, or as «и» plus a combining mark). We convert to the composed form
/// (NFC) first, so the same passphrase always gives the same key.
fn passphrase_bytes(passphrase: &str) -> Zeroizing<Vec<u8>> {
    let composed: String = passphrase.nfc().collect();
    Zeroizing::new(composed.into_bytes())
}

/// Passphrase + salt + settings -> a 32-byte key-encryption key (KEK).
/// Deliberately slow: this is what makes password guessing expensive.
pub fn derive_kek(passphrase: &str, salt: &[u8], params: &KdfParams) -> Result<Key> {
    params.check()?;
    let argon_params = Params::new(params.m_kib, params.t, params.p, Some(KEY_LEN))
        .map_err(|e| Error::BadKdfParams(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(&passphrase_bytes(passphrase), salt, &mut *out)
        .map_err(|e| Error::BadKdfParams(e.to_string()))?;
    Ok(out)
}

/// Derives an independent 32-byte key from `root`, named by `label`.
/// Different labels give unrelated keys.
fn derive_subkey(root: &Key, label: &str) -> Key {
    let hkdf = Hkdf::<Sha256>::new(None, &root[..]);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    hkdf.expand(label.as_bytes(), &mut *out)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

/// Encrypts a 32-byte key with `kek`. `aad` is extra data that is not stored
/// inside the ciphertext but must be identical when unwrapping.
pub fn wrap_key(kek: &Key, secret: &Key, aad: &[u8]) -> Result<([u8; NONCE_LEN], Vec<u8>)> {
    let cipher = XChaCha20Poly1305::new_from_slice(&kek[..]).expect("KEK is 32 bytes");
    let nonce_bytes = random_bytes::<NONCE_LEN>()?;
    let wrapped = cipher
        .encrypt(&XNonce::from(nonce_bytes), Payload { msg: &secret[..], aad })
        .map_err(|_| Error::Unlock)?;
    Ok((nonce_bytes, wrapped))
}

/// The reverse of `wrap_key`. Fails if the KEK, nonce, ciphertext or `aad`
/// differ from what was used to wrap, even by one bit.
pub fn unwrap_key(kek: &Key, nonce: &[u8; NONCE_LEN], wrapped: &[u8], aad: &[u8]) -> Result<Key> {
    let cipher = XChaCha20Poly1305::new_from_slice(&kek[..]).expect("KEK is 32 bytes");
    let plain = cipher
        .decrypt(&XNonce::from(*nonce), Payload { msg: wrapped, aad })
        .map_err(|_| Error::Unlock)?;
    let plain = Zeroizing::new(plain);
    let key: [u8; KEY_LEN] = plain.as_slice().try_into().map_err(|_| Error::Unlock)?;
    Ok(Zeroizing::new(key))
}

// ---------------------------------------------------------------------------
// Key types. Neither derives Debug or Clone, so a key cannot be printed into a
// log by accident and cannot be copied around without somebody writing code
// that does it on purpose.
// ---------------------------------------------------------------------------

/// The random key that protects all data. Never derived from a passphrase.
pub struct MasterKey(Key);

impl MasterKey {
    pub fn generate() -> Result<Self> {
        let master = MasterKey(random_key()?);
        master.lock_memory();
        Ok(master)
    }

    pub(crate) fn from_key(key: Key) -> Self {
        let master = MasterKey(key);
        master.lock_memory();
        master
    }

    pub(crate) fn key(&self) -> &Key {
        &self.0
    }

    /// Key for the SQLCipher database.
    pub fn db_key(&self) -> Key {
        derive_subkey(&self.0, LABEL_DB)
    }

    /// Root key for attachment blobs.
    pub fn attachment_key(&self) -> Key {
        derive_subkey(&self.0, LABEL_ATTACHMENTS)
    }

    /// Only for tests and comparisons.
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// §9: keep the master key's pages out of swap. Best-effort:
    /// a failed mlock never blocks opening a vault.
    fn lock_memory(&self) {
        lock_bytes(&self.0);
    }
}

impl Drop for MasterKey {
    fn drop(&mut self) {
        // Release the pages first; the Zeroizing field then
        // overwrites the key itself as it drops.
        unlock_bytes(&self.0);
    }
}

#[cfg(unix)]
fn lock_bytes(key: &Key) {
    let _ = unsafe { libc::mlock(key.as_ptr() as *const libc::c_void, KEY_LEN) };
}

#[cfg(not(unix))]
fn lock_bytes(_key: &Key) {}

#[cfg(unix)]
fn unlock_bytes(key: &Key) {
    let _ = unsafe { libc::munlock(key.as_ptr() as *const libc::c_void, KEY_LEN) };
}

#[cfg(not(unix))]
fn unlock_bytes(_key: &Key) {}

/// 256 random bits held by the device keychain (Phase 8):
/// the Touch ID slot's credential. The secret itself never
/// leaves the keychain; what the header stores is the master
/// key wrapped under a key derived from it. If the keychain
/// item is gone (profile expired, biometry re-enrolled), the
/// slot is simply dead and the other slots still open.
pub struct DeviceSecret(Key);

impl DeviceSecret {
    pub fn generate() -> Result<Self> {
        Ok(DeviceSecret(random_key()?))
    }

    /// Rebuilds the secret from the bytes the keychain stored.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != KEY_LEN {
            return Err(Error::Unlock);
        }
        let mut raw = [0u8; KEY_LEN];
        raw.copy_from_slice(bytes);
        Ok(DeviceSecret(Zeroizing::new(raw)))
    }

    /// The raw bytes, for the caller to put in the keychain.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0[..]
    }

    pub(crate) fn kek(&self) -> Key {
        derive_subkey(&self.0, LABEL_DEVICE_KEK)
    }
}

/// 256 random bits shown to the user once, as a fallback to the passphrase.
pub struct RecoveryKey(Key);

impl RecoveryKey {
    pub fn generate() -> Result<Self> {
        Ok(RecoveryKey(random_key()?))
    }

    /// The key-encryption key for the recovery slot. The recovery key already
    /// has 256 random bits, so no slow Argon2id step is needed.
    pub(crate) fn kek(&self) -> Key {
        derive_subkey(&self.0, LABEL_RECOVERY_KEK)
    }

    /// Text for the user to write down: 52 Base32 characters in groups of four,
    /// for example `ABCD-EFGH-...`.
    pub fn to_text(&self) -> String {
        let encoded = BASE32_NOPAD.encode(&self.0[..]);
        let chars: Vec<char> = encoded.chars().collect();
        chars
            .chunks(4)
            .map(|group| group.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Reads the text back. Ignores dashes, spaces and letter case.
    pub fn from_text(text: &str) -> Result<Self> {
        let cleaned: String = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .flat_map(|c| c.to_uppercase())
            .collect();
        let bytes = Zeroizing::new(
            BASE32_NOPAD
                .decode(cleaned.as_bytes())
                .map_err(|_| Error::InvalidRecoveryKey("not valid Base32".into()))?,
        );
        let key: [u8; KEY_LEN] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::InvalidRecoveryKey("wrong length".into()))?;
        Ok(RecoveryKey(Zeroizing::new(key)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run it by hand (it needs seconds and a lot of memory):
    ///   cargo test -p soirvu-core --release calibration -- --ignored --nocapture
    #[test]
    #[ignore]
    fn calibration_report() {
        let started = Instant::now();
        let params = KdfParams::calibrate(Duration::from_millis(500)).unwrap();
        println!("calibration took {:?}, chose {params:?}", started.elapsed());

        let started = Instant::now();
        derive_kek("some passphrase", &[1u8; SALT_LEN], &params).unwrap();
        println!("one unlock with these settings: {:?}", started.elapsed());
    }
}
