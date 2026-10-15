//! §9 logging audit, as a test: opening a vault whose database
//! fails its HMAC check must print NOTHING. SQLCipher's own
//! logger and SQLite's sqlite3_log both write to stderr by
//! default; the messages carry page numbers, not content, but
//! the design says silence them when silence is achievable.
//!
//! A test cannot read its own process's stderr, so the scenario
//! runs in a child copy of this test binary.

use std::io::Write;
use std::time::Duration;

use soirvu_core::crypto::KdfParams;
use soirvu_core::Vault;

const CHILD_ENV: &str = "SOIRVU_SILENCE_CHILD";

#[test]
fn database_errors_are_not_printed() {
    if std::env::var_os(CHILD_ENV).is_some() {
        child_scenario();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "database_errors_are_not_printed",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("scenario: open failed as expected"),
        "child scenario did not run as expected:\n{stdout}\n{stderr}"
    );
    for needle in ["sqlcipher", "ERROR CORE", "hmac"] {
        assert!(
            !stderr.contains(needle),
            "stderr leaked a database diagnostic ({needle}):\n{stderr}"
        );
    }
}

fn child_scenario() {
    let dir = tempfile::tempdir().unwrap();
    let vault_dir = dir.path().join("v");
    let (vault, _recovery) = Vault::create(
        &vault_dir,
        "correct horse",
        KdfParams::calibrate(Duration::from_millis(5)).unwrap(),
    )
    .unwrap();
    vault.lock();
    // Corrupt page 1 of the encrypted database: its HMAC check
    // will fail on the first read inside open.
    let db_path = vault_dir.join("soirvu.db");
    let mut bytes = std::fs::read(&db_path).unwrap();
    bytes[100] ^= 0xFF;
    std::fs::write(&db_path, &bytes).unwrap();
    let failed = Vault::open_with_passphrase(&vault_dir, "correct horse").is_err();
    println!(
        "scenario: {}",
        if failed { "open failed as expected" } else { "open unexpectedly succeeded" }
    );
    std::io::stdout().flush().unwrap();
}
