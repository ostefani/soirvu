//! Cross-OS vault exchange (DESIGN §12, Phase 9 step 1):
//! a vault created on one OS must open on every other OS.
//!
//! Driven by CI through environment variables; with neither
//! set, the test is a no-op so ordinary `cargo test` runs
//! stay self-contained.
//!
//!   SOIRVU_MAKE_FIXTURE=<dir>  create the fixture vault
//!   SOIRVU_OPEN_FIXTURE=<dir>  open and verify it
//!
//! The fixture deliberately includes a device slot (Phase
//! 8): the header must parse everywhere, and the passphrase
//! slot must open the vault even where no keychain exists.

use std::path::PathBuf;

use soirvu_core::entries::EntryDraft;
use soirvu_core::crypto::KdfParams;
use soirvu_core::Vault;

const PASSPHRASE: &str = "cross-os fixture passphrase";

#[test]
fn cross_os_fixture() {
    if let Ok(dir) = std::env::var("SOIRVU_MAKE_FIXTURE") {
        let dir = PathBuf::from(dir);
        let (mut vault, _recovery) =
            Vault::create(&dir, PASSPHRASE, KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }).unwrap();
        vault
            .create_entry(&EntryDraft {
                entry_date: "2026-10-16".into(),
                title: Some("Cross-OS fixture".into()),
                body_md: "Created on one OS, opened on another.".into(),
                mood: None,
                tz: None,
                tags: vec!["fixture".into()],
            })
            .unwrap();
        // A device slot in the header, on purpose.
        vault.enroll_device().unwrap();
        vault.lock();
        return;
    }
    if let Ok(dir) = std::env::var("SOIRVU_OPEN_FIXTURE") {
        let dir = PathBuf::from(dir);
        let vault = Vault::open_with_passphrase(&dir, PASSPHRASE).unwrap();
        assert!(vault.has_device_slot());
        let report = vault.verify().unwrap();
        assert_eq!(report.entries, 1);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }
}
