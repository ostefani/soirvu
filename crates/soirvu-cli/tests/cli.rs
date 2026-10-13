//! End-to-end tests: run the real `soirvu-cli` binary against real vaults.
//! Vaults with content are made through `soirvu-core` directly — the CLI
//! has no "add entry" command on purpose; it is a recovery tool.

use std::fs;
use std::process::{Command, Output};

use soirvu_core::crypto::KdfParams;
use soirvu_core::entries::EntryDraft;
use soirvu_core::Vault;
use tempfile::TempDir;

const PASSPHRASE: &str = "correct horse battery staple";

/// The weakest settings a REAL build accepts. The core's own unit tests
/// use much weaker ones, but those only pass inside the core: its low
/// floors are `#[cfg(test)]`, and a test of this crate compiles the core
/// as a normal dependency, with the production floors (DESIGN §5).
/// So vaults made here cost a real Argon2id run each. Still fast enough.
fn fast() -> KdfParams {
    KdfParams {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    }
}

fn cli(args: &[&str], passphrase: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soirvu-cli"))
        .args(args)
        .env("SOIRVU_PASSPHRASE", passphrase)
        .output()
        .expect("run soirvu-cli")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A vault with one entry and one attachment, made with the core.
/// Returns the temp dir, the vault dir, and the recovery key text.
fn vault_with_content() -> (TempDir, std::path::PathBuf, String) {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("v");
    let (mut vault, recovery) = Vault::create(&dir, PASSPHRASE, fast()).unwrap();
    let entry = vault
        .create_entry(&EntryDraft {
            entry_date: "2026-10-13".into(),
            title: Some("Проверка".into()),
            body_md: "текст про книгу".into(),
            ..Default::default()
        })
        .unwrap();
    vault
        .add_attachment(&entry.id, "a.bin", "application/octet-stream", b"bytes")
        .unwrap();
    vault.lock();
    (tmp, dir, recovery.to_text())
}

#[test]
fn create_then_header_info_then_verify() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("v");
    let dir_s = dir.to_string_lossy().into_owned();

    // Note: `create` calibrates Argon2id on this machine, so it is slower
    // than the other commands. That is the design (§5), not a bug.
    let created = cli(&["create", &dir_s], PASSPHRASE);
    assert!(created.status.success(), "{}", stdout(&created));
    let text = stdout(&created);
    assert!(text.contains("Vault created:"));
    assert!(text.contains("Recovery key")); // shown once, at creation

    let info = cli(&["header-info", &dir_s], PASSPHRASE);
    assert!(info.status.success());
    let text = stdout(&info);
    assert!(text.contains("format version: 1"));
    assert!(text.contains("slot: passphrase"));
    assert!(text.contains("slot: recovery key"));

    let checked = cli(&["verify", &dir_s], PASSPHRASE);
    assert!(checked.status.success(), "{}", stdout(&checked));
    assert!(stdout(&checked).contains("verify: OK"));
}

#[test]
fn verify_and_export_work_with_the_recovery_key() {
    let (_tmp, dir, recovery_text) = vault_with_content();
    let dir_s = dir.to_string_lossy().into_owned();

    // A wrong passphrase in the environment must not matter: --recovery
    // replaces the passphrase entirely.
    let checked = cli(&["verify", &dir_s, "--recovery", &recovery_text], "wrong");
    assert!(checked.status.success(), "{}", stdout(&checked));
    assert!(stdout(&checked).contains("entries: 1"));
    assert!(stdout(&checked).contains("attachments: 1"));

    let out = _tmp.path().join("out");
    let out_s = out.to_string_lossy().into_owned();
    let exported = cli(
        &["export", &dir_s, &out_s, "--recovery", &recovery_text],
        "wrong",
    );
    assert!(exported.status.success(), "{}", stdout(&exported));
    assert!(stdout(&exported).contains("exported 1 entries and 1 attachments"));

    let year = out.join("2026");
    let files: Vec<_> = fs::read_dir(&year).unwrap().collect();
    assert_eq!(files.len(), 1);
    let md = fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
    assert!(md.contains("текст про книгу"));
    assert!(md.contains("title: \"Проверка\""));
}

#[test]
fn a_wrong_passphrase_fails() {
    let (_tmp, dir, _recovery) = vault_with_content();
    let dir_s = dir.to_string_lossy().into_owned();
    let checked = cli(&["verify", &dir_s], "not the passphrase");
    assert_eq!(checked.status.code(), Some(1));
}

#[test]
fn create_refuses_a_folder_that_is_not_empty() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("mine.txt"), b"precious").unwrap();
    let dir_s = tmp.path().to_string_lossy().into_owned();
    let created = cli(&["create", &dir_s], PASSPHRASE);
    assert_eq!(created.status.code(), Some(1));
    assert_eq!(fs::read(tmp.path().join("mine.txt")).unwrap(), b"precious");
}

#[test]
fn no_arguments_is_a_usage_error() {
    let out = cli(&[], PASSPHRASE);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("soirvu-cli create"), "{err}");
}

#[test]
fn verify_reports_a_damaged_vault() {
    let (_tmp, dir, _recovery) = vault_with_content();
    // Damage the blob: flip its last byte.
    let attachments = dir.join("attachments");
    let shard = fs::read_dir(&attachments).unwrap().next().unwrap().unwrap().path();
    let blob = fs::read_dir(&shard).unwrap().next().unwrap().unwrap().path();
    let mut bytes = fs::read(&blob).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    fs::write(&blob, bytes).unwrap();

    let dir_s = dir.to_string_lossy().into_owned();
    let checked = cli(&["verify", &dir_s], PASSPHRASE);
    assert_eq!(checked.status.code(), Some(1));
    assert!(stdout(&checked).contains("problem:"), "{}", stdout(&checked));
}
