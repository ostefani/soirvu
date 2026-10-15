//! The vault header: `vault.header.json`.
//!
//! The header is plain JSON and holds no secrets. It stores the master key
//! wrapped (encrypted) once per "slot": one slot opens with the passphrase,
//! another with the recovery key. More slots (Touch ID, ...) can be added later.

use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};

use crate::crypto::{
    KdfParams, MasterKey, NONCE_LEN, RecoveryKey, SALT_LEN, WRAPPED_LEN, derive_kek, random_bytes,
    unwrap_key, wrap_key,
};
use crate::error::{Error, Result};

pub const FORMAT_VERSION: u32 = 1;
/// Names the exact set of algorithms, so they can be changed later without guessing.
pub const SUITE: &str = "argon2id-xchacha20poly1305-hkdfsha256";
const VAULT_ID_LEN: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultHeader {
    pub format_version: u32,
    pub suite: String,
    /// 16 random bytes as 32 lowercase hex characters.
    pub vault_id: String,
    pub slots: Vec<Slot>,
}

/// One way to open the vault. Byte fields are lowercase hex strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Slot {
    /// Master key encrypted under a key stretched from the passphrase by Argon2id.
    Passphrase {
        kdf: KdfParams,
        salt: String,
        nonce: String,
        wrapped_key: String,
    },
    /// Master key encrypted under a key derived from the recovery key.
    Recovery { nonce: String, wrapped_key: String },
}

// ---------------------------------------------------------------------------
// AAD ("additional authenticated data").
//
// When a slot is wrapped, we also feed in a text made of the header fields that
// must not change. It is not stored in the ciphertext; unwrapping only works if
// the text built from the CURRENT header is identical. So changing the vault id,
// the format, or the Argon2id settings makes unwrapping fail, instead of
// silently weakening the vault.
// ---------------------------------------------------------------------------
fn aad(format_version: u32, suite: &str, vault_id: &str, slot_description: &str) -> Vec<u8> {
    format!(
        "soirvu-header\nformat={format_version}\nsuite={suite}\nvault_id={vault_id}\nslot={slot_description}"
    )
    .into_bytes()
}

fn passphrase_description(kdf: &KdfParams) -> String {
    format!("passphrase;m={};t={};p={}", kdf.m_kib, kdf.t, kdf.p)
}

const RECOVERY_DESCRIPTION: &str = "recovery";

/// Hex text -> exactly N bytes.
fn decode_fixed<const N: usize>(text: &str, field: &str) -> Result<[u8; N]> {
    let bytes = HEXLOWER
        .decode(text.as_bytes())
        .map_err(|_| Error::InvalidHeader(format!("{field} is not lowercase hex")))?;
    bytes
        .try_into()
        .map_err(|_| Error::InvalidHeader(format!("{field} has the wrong length")))
}

impl VaultHeader {
    /// Creates a new vault: a fresh random master key, locked by the passphrase
    /// and by a newly generated recovery key.
    ///
    /// Returns the header (to save as a file), the recovery key (to show to the
    /// user ONCE) and the master key (to open the database right away).
    pub fn create(passphrase: &str, kdf: KdfParams) -> Result<(VaultHeader, RecoveryKey, MasterKey)> {
        if passphrase.is_empty() {
            return Err(Error::EmptyPassphrase);
        }
        kdf.check()?;

        let master = MasterKey::generate()?;
        let recovery = RecoveryKey::generate()?;
        let vault_id = HEXLOWER.encode(&random_bytes::<VAULT_ID_LEN>()?);

        // Passphrase slot.
        let salt = random_bytes::<SALT_LEN>()?;
        let kek = derive_kek(passphrase, &salt, &kdf)?;
        let slot_aad = aad(FORMAT_VERSION, SUITE, &vault_id, &passphrase_description(&kdf));
        let (nonce, wrapped) = wrap_key(&kek, master.key(), &slot_aad)?;
        let passphrase_slot = Slot::Passphrase {
            kdf,
            salt: HEXLOWER.encode(&salt),
            nonce: HEXLOWER.encode(&nonce),
            wrapped_key: HEXLOWER.encode(&wrapped),
        };

        // Recovery slot.
        let slot_aad = aad(FORMAT_VERSION, SUITE, &vault_id, RECOVERY_DESCRIPTION);
        let (nonce, wrapped) = wrap_key(&recovery.kek(), master.key(), &slot_aad)?;
        let recovery_slot = Slot::Recovery {
            nonce: HEXLOWER.encode(&nonce),
            wrapped_key: HEXLOWER.encode(&wrapped),
        };

        let header = VaultHeader {
            format_version: FORMAT_VERSION,
            suite: SUITE.to_string(),
            vault_id,
            slots: vec![passphrase_slot, recovery_slot],
        };
        Ok((header, recovery, master))
    }

    /// A new identity for a copy of this vault (an imported
    /// backup): a fresh vault_id, the SAME master key re-wrapped
    /// into both slots under the new id (the slot AAD binds the
    /// id, so the old wrappings cannot survive), the passphrase
    /// unchanged — and a NEW recovery key, because the old
    /// recovery slot cannot be re-wrapped without the old
    /// recovery key, and separate diaries should not share one.
    /// The caller has already proven the passphrase by opening
    /// the vault.
    pub fn fork(&self, passphrase: &str, master: &MasterKey) -> Result<(VaultHeader, RecoveryKey)> {
        let kdf = match &self.slots[0] {
            Slot::Passphrase { kdf, .. } => *kdf,
            _ => return Err(Error::InvalidHeader("first slot is not a passphrase slot".into())),
        };
        let recovery = RecoveryKey::generate()?;
        let vault_id = HEXLOWER.encode(&random_bytes::<VAULT_ID_LEN>()?);

        let salt = random_bytes::<SALT_LEN>()?;
        let kek = derive_kek(passphrase, &salt, &kdf)?;
        let slot_aad = aad(FORMAT_VERSION, SUITE, &vault_id, &passphrase_description(&kdf));
        let (nonce, wrapped) = wrap_key(&kek, master.key(), &slot_aad)?;
        let passphrase_slot = Slot::Passphrase {
            kdf,
            salt: HEXLOWER.encode(&salt),
            nonce: HEXLOWER.encode(&nonce),
            wrapped_key: HEXLOWER.encode(&wrapped),
        };

        let slot_aad = aad(FORMAT_VERSION, SUITE, &vault_id, RECOVERY_DESCRIPTION);
        let (nonce, wrapped) = wrap_key(&recovery.kek(), master.key(), &slot_aad)?;
        let recovery_slot = Slot::Recovery {
            nonce: HEXLOWER.encode(&nonce),
            wrapped_key: HEXLOWER.encode(&wrapped),
        };

        let header = VaultHeader {
            format_version: FORMAT_VERSION,
            suite: SUITE.to_string(),
            vault_id,
            slots: vec![passphrase_slot, recovery_slot],
        };
        Ok((header, recovery))
    }

    /// Checks everything that can be checked without a secret.
    pub fn validate(&self) -> Result<()> {
        if self.format_version != FORMAT_VERSION {
            return Err(Error::UnsupportedFormat(format!(
                "format version {}",
                self.format_version
            )));
        }
        if self.suite != SUITE {
            return Err(Error::UnsupportedFormat(format!("suite {}", self.suite)));
        }
        decode_fixed::<VAULT_ID_LEN>(&self.vault_id, "vault_id")?;
        if self.slots.is_empty() {
            return Err(Error::InvalidHeader("no key slots".into()));
        }
        for slot in &self.slots {
            match slot {
                Slot::Passphrase { kdf, salt, nonce, wrapped_key } => {
                    kdf.check()?;
                    decode_fixed::<SALT_LEN>(salt, "salt")?;
                    decode_fixed::<NONCE_LEN>(nonce, "nonce")?;
                    decode_fixed::<WRAPPED_LEN>(wrapped_key, "wrapped_key")?;
                }
                Slot::Recovery { nonce, wrapped_key } => {
                    decode_fixed::<NONCE_LEN>(nonce, "nonce")?;
                    decode_fixed::<WRAPPED_LEN>(wrapped_key, "wrapped_key")?;
                }
            }
        }
        Ok(())
    }

    /// Opens the vault with the passphrase.
    pub fn unlock_with_passphrase(&self, passphrase: &str) -> Result<MasterKey> {
        self.validate()?;
        for slot in &self.slots {
            if let Slot::Passphrase { kdf, salt, nonce, wrapped_key } = slot {
                let salt = decode_fixed::<SALT_LEN>(salt, "salt")?;
                let nonce = decode_fixed::<NONCE_LEN>(nonce, "nonce")?;
                let wrapped = decode_fixed::<WRAPPED_LEN>(wrapped_key, "wrapped_key")?;
                let slot_aad = aad(
                    self.format_version,
                    &self.suite,
                    &self.vault_id,
                    &passphrase_description(kdf),
                );
                let kek = derive_kek(passphrase, &salt, kdf)?;
                if let Ok(key) = unwrap_key(&kek, &nonce, &wrapped, &slot_aad) {
                    return Ok(MasterKey::from_key(key));
                }
            }
        }
        Err(Error::Unlock)
    }

    /// Opens the vault with the recovery key.
    pub fn unlock_with_recovery(&self, recovery: &RecoveryKey) -> Result<MasterKey> {
        self.validate()?;
        for slot in &self.slots {
            if let Slot::Recovery { nonce, wrapped_key } = slot {
                let nonce = decode_fixed::<NONCE_LEN>(nonce, "nonce")?;
                let wrapped = decode_fixed::<WRAPPED_LEN>(wrapped_key, "wrapped_key")?;
                let slot_aad = aad(
                    self.format_version,
                    &self.suite,
                    &self.vault_id,
                    RECOVERY_DESCRIPTION,
                );
                if let Ok(key) = unwrap_key(&recovery.kek(), &nonce, &wrapped, &slot_aad) {
                    return Ok(MasterKey::from_key(key));
                }
            }
        }
        Err(Error::Unlock)
    }

    /// Compact JSON (no extra whitespace) ready to be written to `vault.header.json`.
    pub fn to_json(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }

    /// Reads and validates a header file.
    pub fn from_json(bytes: &[u8]) -> Result<VaultHeader> {
        let header: VaultHeader = serde_json::from_slice(bytes)?;
        header.validate()?;
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSPHRASE: &str = "correct horse battery staple";

    /// Cheap Argon2id settings so tests are instant. (Test builds accept them;
    /// real builds enforce at least 64 MiB and 3 passes.)
    fn fast() -> KdfParams {
        KdfParams { m_kib: 8, t: 1, p: 1 }
    }

    fn new_vault() -> (VaultHeader, RecoveryKey, MasterKey) {
        VaultHeader::create(PASSPHRASE, fast()).expect("create")
    }

    #[test]
    fn passphrase_opens_the_vault_and_subkeys_differ() {
        let (header, _recovery, master) = new_vault();
        let opened = header.unlock_with_passphrase(PASSPHRASE).expect("unlock");
        assert!(opened.as_bytes() == master.as_bytes());
        // The database key and the attachment key come from the same master key
        // but must be different from each other and from the master key itself.
        assert!(master.db_key()[..] != master.attachment_key()[..]);
        assert!(master.db_key()[..] != master.as_bytes()[..]);
        // Same input, same output.
        assert!(master.db_key()[..] == opened.db_key()[..]);
    }

    #[test]
    fn recovery_key_opens_the_vault() {
        let (header, recovery, master) = new_vault();
        let text = recovery.to_text();
        assert_eq!(text.len(), 52 + 12); // 52 characters + 12 dashes
        // Typing it in lowercase, with spaces instead of dashes, still works.
        let typed = text.to_lowercase().replace('-', " ");
        let parsed = RecoveryKey::from_text(&typed).expect("parse");
        let opened = header.unlock_with_recovery(&parsed).expect("unlock");
        assert!(opened.as_bytes() == master.as_bytes());
    }

    #[test]
    fn wrong_secrets_are_refused() {
        let (header, _recovery, _master) = new_vault();
        assert!(matches!(
            header.unlock_with_passphrase("wrong passphrase"),
            Err(Error::Unlock)
        ));
        let other = RecoveryKey::generate().unwrap();
        assert!(matches!(header.unlock_with_recovery(&other), Err(Error::Unlock)));
    }

    #[test]
    fn bad_recovery_text_is_refused() {
        assert!(matches!(
            RecoveryKey::from_text("ABCD-EFGH"),
            Err(Error::InvalidRecoveryKey(_))
        ));
        assert!(matches!(
            RecoveryKey::from_text("!!!!-????"),
            Err(Error::InvalidRecoveryKey(_))
        ));
    }

    #[test]
    fn empty_passphrase_is_refused() {
        assert!(matches!(
            VaultHeader::create("", fast()),
            Err(Error::EmptyPassphrase)
        ));
    }

    #[test]
    fn header_survives_a_trip_through_json() {
        let (header, _recovery, master) = new_vault();
        let bytes = header.to_json().unwrap();
        let reread = VaultHeader::from_json(&bytes).unwrap();
        let opened = reread.unlock_with_passphrase(PASSPHRASE).unwrap();
        assert!(opened.as_bytes() == master.as_bytes());
        // Print it once so you can look at the file format: cargo test -- --nocapture
        println!("{}", String::from_utf8_lossy(&bytes));
    }

    /// Flip one bit in every byte of the header file, one byte at a time.
    /// Each damaged header must be caught: it must fail to parse, or at least
    /// one of the two slots must refuse to unlock. And whenever a slot DOES
    /// unlock, it must give the original master key, never some other key.
    #[test]
    fn every_modified_header_byte_is_detected() {
        let (header, recovery, master) = new_vault();
        let recovery_text = recovery.to_text();
        let original = header.to_json().unwrap();

        for i in 0..original.len() {
            let mut damaged = original.clone();
            damaged[i] ^= 0x01;

            let Ok(damaged_header) = VaultHeader::from_json(&damaged) else {
                continue; // refused while reading: detected
            };
            let by_passphrase = damaged_header.unlock_with_passphrase(PASSPHRASE);
            let by_recovery = damaged_header
                .unlock_with_recovery(&RecoveryKey::from_text(&recovery_text).unwrap());

            if let Ok(key) = &by_passphrase {
                assert!(key.as_bytes() == master.as_bytes(), "byte {i}: different key");
            }
            if let Ok(key) = &by_recovery {
                assert!(key.as_bytes() == master.as_bytes(), "byte {i}: different key");
            }
            assert!(
                by_passphrase.is_err() || by_recovery.is_err(),
                "byte {i} was changed and nothing noticed"
            );
        }
    }

    #[test]
    fn weakening_the_kdf_settings_is_refused() {
        let (header, _recovery, _master) = new_vault();

        // Far below the allowed minimum: refused as soon as the file is read.
        let mut weak = header.clone();
        if let Slot::Passphrase { kdf, .. } = &mut weak.slots[0] {
            kdf.m_kib = 1;
        }
        assert!(matches!(
            VaultHeader::from_json(&weak.to_json().unwrap()),
            Err(Error::BadKdfParams(_))
        ));

        // Absurdly large: refused too (otherwise unlock could hang or run out of memory).
        let mut huge = header.clone();
        if let Slot::Passphrase { kdf, .. } = &mut huge.slots[0] {
            kdf.m_kib = u32::MAX;
        }
        assert!(matches!(huge.validate(), Err(Error::BadKdfParams(_))));

        // Allowed but different from what the slot was created with: this passes
        // the range check, and the authentication then fails.
        let mut changed = header.clone();
        if let Slot::Passphrase { kdf, .. } = &mut changed.slots[0] {
            kdf.t += 1;
        }
        assert!(matches!(
            changed.unlock_with_passphrase(PASSPHRASE),
            Err(Error::Unlock)
        ));
    }

    #[test]
    fn slots_are_bound_to_their_vault() {
        let (mut header, recovery, _master) = new_vault();
        header.vault_id = "00".repeat(VAULT_ID_LEN);
        assert!(matches!(
            header.unlock_with_passphrase(PASSPHRASE),
            Err(Error::Unlock)
        ));
        assert!(matches!(header.unlock_with_recovery(&recovery), Err(Error::Unlock)));
    }

    #[test]
    fn the_same_passphrase_written_two_ways_is_the_same_passphrase() {
        // «й» as one character (U+0439) and as «и» + combining breve (U+0438 U+0306).
        let composed = "пароль-й";
        let decomposed = "пароль-\u{438}\u{306}";
        assert_ne!(composed.as_bytes(), decomposed.as_bytes());

        let (header, _recovery, master) = VaultHeader::create(decomposed, fast()).unwrap();
        let opened = header.unlock_with_passphrase(composed).unwrap();
        assert!(opened.as_bytes() == master.as_bytes());
    }
}
