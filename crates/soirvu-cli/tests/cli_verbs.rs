//! End-to-end checks for the hand-parsed verb surface.
//! The two verbs added with the ack work (fix8) shipped
//! with a wrong arity guard on one of them, and nothing
//! in the core suite could see it — these run the real
//! binary, bare and with --recovery.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PASS: &str = "cli-verbs-test-passphrase";

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("soirvu-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(work: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soirvu-cli"))
        .args(args)
        .env("SOIRVU_PASSPHRASE", PASS)
        .current_dir(work)
        .output()
        .unwrap()
}

/// Creates a vault under `work`; returns its path and
/// the recovery key text from the create output.
fn create_vault(work: &Path) -> (String, String) {
    let vault = work.join("vault");
    let out = run(work, &["create", vault.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let mut lines = stdout.lines();
    let recovery = loop {
        let line = lines.next().expect("recovery banner line");
        if line.starts_with("Recovery key") {
            break lines.next().expect("recovery key line").trim().to_string();
        }
    };
    (vault.to_str().unwrap().to_string(), recovery)
}

#[test]
fn sync_dismiss_ack_ahead_runs_bare() {
    let work = scratch("dismiss-bare");
    let (vault, _recovery) = create_vault(&work);
    let out = run(&work, &["sync-dismiss-ack-ahead", &vault]);
    assert!(
        out.status.success(),
        "bare dismiss failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("flag cleared"));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn sync_dismiss_ack_ahead_runs_with_recovery() {
    let work = scratch("dismiss-recovery");
    let (vault, recovery) = create_vault(&work);
    let out = run(
        &work,
        &["sync-dismiss-ack-ahead", &vault, "--recovery", &recovery],
    );
    assert!(
        out.status.success(),
        "recovery dismiss failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn sync_reemit_runs_bare_and_with_recovery() {
    let work = scratch("reemit");
    let (vault, recovery) = create_vault(&work);
    let sync = work.join("sync");
    let sync = sync.to_str().unwrap();
    let out = run(&work, &["sync-reemit", &vault, sync]);
    assert!(
        out.status.success(),
        "bare re-emit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("nothing to re-emit"));
    let out = run(&work, &["sync-reemit", &vault, sync, "--recovery", &recovery]);
    assert!(
        out.status.success(),
        "recovery re-emit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&work);
}
