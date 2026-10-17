//! soirvu-cli — the recovery escape hatch (DESIGN §2, §8).
//!
//! It runs the same `soirvu-core` code as the app, so a vault can be
//! created, checked and exported even if the GUI is broken or gone.
//! No argument-parsing crate: the surface is four commands, parsed by hand.
//!
//! Commands:
//!   soirvu-cli create <vault-dir>
//!   soirvu-cli header-info <vault-dir>
//!   soirvu-cli verify <vault-dir> [--recovery <key>]
//!   soirvu-cli export <vault-dir> <out-dir> [--recovery <key>]
//!   soirvu-cli sync-emit <vault-dir> <sync-dir> [--recovery <key>]
//!   soirvu-cli sync-apply <vault-dir> <sync-dir> [--recovery <key>]
//!
//! The passphrase comes from the SOIRVU_PASSPHRASE environment variable,
//! or is asked for at the terminal (with echo off). `--recovery` takes the
//! recovery key text instead of a passphrase.
//!
//! Exit codes: 0 = ok, 1 = the command failed or verify found problems,
//! 2 = the command line itself was wrong.

use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use soirvu_core::crypto::{KdfParams, RecoveryKey};
use soirvu_core::vault::VaultHeader;
use soirvu_core::{Error, Vault};

const HEADER_FILE: &str = "vault.header.json";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::from(0),
        Err(RunError::Usage) => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(RunError::Failed(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

enum RunError {
    Usage,
    Failed(String),
}

fn failed(error: Error) -> RunError {
    RunError::Failed(error.to_string())
}

const USAGE: &str = "\
soirvu-cli — create, check and export a Soirvu vault without the app

  soirvu-cli create <vault-dir>
  soirvu-cli header-info <vault-dir>
  soirvu-cli verify <vault-dir> [--recovery <key>]
  soirvu-cli export <vault-dir> <out-dir> [--recovery <key>]
  soirvu-cli sync-emit <vault-dir> <sync-dir> [--recovery <key>]
  soirvu-cli sync-apply <vault-dir> <sync-dir> [--recovery <key>]

The passphrase is read from SOIRVU_PASSPHRASE, or asked for at the
terminal with echo off. Export writes PLAINTEXT — see DESIGN §8.
";

fn run(args: &[String]) -> Result<(), RunError> {
    let (cmd, rest) = args.split_first().ok_or(RunError::Usage)?;
    match cmd.as_str() {
        "create" if rest.len() == 1 => create(Path::new(&rest[0])),
        "header-info" if rest.len() == 1 => header_info(Path::new(&rest[0])),
        "verify" if !rest.is_empty() => {
            let (dirs, recovery) = open_args(rest)?;
            if dirs.len() != 1 {
                return Err(RunError::Usage);
            }
            verify(Path::new(&dirs[0]), recovery)
        }
        "export" if rest.len() >= 2 => {
            let (dirs, recovery) = open_args(rest)?;
            if dirs.len() != 2 {
                return Err(RunError::Usage);
            }
            export(Path::new(&dirs[0]), Path::new(&dirs[1]), recovery)
        }
        "sync-emit" if rest.len() >= 2 => {
            let (dirs, recovery) = open_args(rest)?;
            if dirs.len() != 2 {
                return Err(RunError::Usage);
            }
            sync_emit(Path::new(&dirs[0]), Path::new(&dirs[1]), recovery)
        }
        "sync-apply" if rest.len() >= 2 => {
            let (dirs, recovery) = open_args(rest)?;
            if dirs.len() != 2 {
                return Err(RunError::Usage);
            }
            sync_apply(Path::new(&dirs[0]), Path::new(&dirs[1]), recovery)
        }
        _ => Err(RunError::Usage),
    }
}

/// Splits `[paths..., --recovery <key>]` into the paths and the key text.
fn open_args(args: &[String]) -> Result<(&[String], Option<&str>), RunError> {
    if args.len() >= 2 && args[args.len() - 2] == "--recovery" {
        Ok((&args[..args.len() - 2], Some(args[args.len() - 1].as_str())))
    } else if args.iter().any(|a| a == "--recovery") {
        Err(RunError::Usage)
    } else {
        Ok((args, None))
    }
}

fn create(dir: &Path) -> Result<(), RunError> {
    let passphrase = read_passphrase("New passphrase: ")?;
    if passphrase.is_empty() {
        return Err(RunError::Failed(Error::EmptyPassphrase.to_string()));
    }
    // Calibrated on this machine, about 0.5–1 s per unlock (DESIGN §5).
    let kdf = KdfParams::calibrate(Duration::from_millis(750)).map_err(failed)?;
    let (vault, recovery) = Vault::create(dir, &passphrase, kdf).map_err(failed)?;
    let vault_id = vault.header().vault_id.clone();
    vault.lock();
    println!("Vault created: {vault_id}");
    println!("Recovery key — write it down now, it is shown only once:");
    println!("{}", recovery.to_text());
    Ok(())
}

fn header_info(dir: &Path) -> Result<(), RunError> {
    let bytes = std::fs::read(dir.join(HEADER_FILE))
        .map_err(|e| RunError::Failed(format!("cannot read the vault header: {e}")))?;
    let header = VaultHeader::from_json(&bytes).map_err(failed)?;
    println!("format version: {}", header.format_version);
    println!("suite: {}", header.suite);
    println!("vault id: {}", header.vault_id);
    for slot in &header.slots {
        match slot {
            soirvu_core::vault::Slot::Passphrase { kdf, .. } => {
                println!(
                    "slot: passphrase (Argon2id, m = {} KiB, t = {}, p = {})",
                    kdf.m_kib, kdf.t, kdf.p
                );
            }
            soirvu_core::vault::Slot::Recovery { .. } => println!("slot: recovery key"),
            soirvu_core::vault::Slot::Device { .. } => println!("slot: device (Touch ID)"),
        }
    }
    Ok(())
}

fn open(dir: &Path, recovery: Option<&str>) -> Result<Vault, RunError> {
    match recovery {
        Some(text) => {
            let key = RecoveryKey::from_text(text).map_err(failed)?;
            Vault::open_with_recovery(dir, &key).map_err(failed)
        }
        None => {
            let passphrase = read_passphrase("Passphrase: ")?;
            Vault::open_with_passphrase(dir, &passphrase).map_err(failed)
        }
    }
}

fn verify(dir: &Path, recovery: Option<&str>) -> Result<(), RunError> {
    let vault = open(dir, recovery)?;
    let report = vault.verify().map_err(failed)?;
    println!(
        "entries: {} ({} deleted), attachments: {} ({} bytes)",
        report.entries, report.tombstones, report.attachments, report.attachment_bytes
    );
    if report.ok() {
        println!("verify: OK");
        Ok(())
    } else {
        for problem in &report.problems {
            println!("problem: {problem}");
        }
        Err(RunError::Failed(format!(
            "verify found {} problem(s)",
            report.problems.len()
        )))
    }
}

fn export(dir: &Path, out: &Path, recovery: Option<&str>) -> Result<(), RunError> {
    eprintln!("warning: export writes your diary as PLAINTEXT files.");
    let vault = open(dir, recovery)?;
    let report = vault.export_markdown(out).map_err(failed)?;
    println!(
        "exported {} entries and {} attachments to {}",
        report.entries,
        report.attachments,
        report.dir.display()
    );
    Ok(())
}

/// Passphrase from SOIRVU_PASSPHRASE, else a terminal prompt with echo off
/// (via `stty`; if that fails the typed text is visible), else a line from
/// standard input (for scripts that pipe it).
fn read_passphrase(prompt: &str) -> Result<String, RunError> {
    if let Ok(passphrase) = env::var("SOIRVU_PASSPHRASE") {
        return Ok(passphrase);
    }
    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.is_terminal() {
        eprint!("{prompt}");
        let _ = io::stderr().flush();
        let echo_off = std::process::Command::new("stty")
            .arg("-echo")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let read = stdin.lock().read_line(&mut line);
        if echo_off {
            let _ = std::process::Command::new("stty").arg("echo").status();
            eprintln!();
        }
        read.map_err(|e| RunError::Failed(e.to_string()))?;
    } else {
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| RunError::Failed(e.to_string()))?;
    }
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

fn sync_emit(dir: &Path, sync_dir: &Path, recovery: Option<&str>) -> Result<(), RunError> {
    let mut vault = open(dir, recovery)?;
    match vault.sync_emit(sync_dir).map_err(failed)? {
        Some(info) => {
            println!(
                "bundle: {} (lseq {}..={}, {} entries, {} attachments)",
                info.file_name, info.from_lseq, info.to_lseq, info.entries, info.attachments
            );
            Ok(())
        }
        None => {
            println!("nothing new to emit");
            Ok(())
        }
    }
}

fn sync_apply(dir: &Path, sync_dir: &Path, recovery: Option<&str>) -> Result<(), RunError> {
    let mut vault = open(dir, recovery)?;
    let report = vault.sync_apply(sync_dir).map_err(failed)?;
    println!(
        "bundles: {} applied of {} seen; entries: {} applied, {} conflict copies; attachments: {} applied",
        report.bundles_applied,
        report.bundles_seen,
        report.entries_applied,
        report.entries_conflicted,
        report.attachments_applied
    );
    for problem in &report.problems {
        println!("problem: {problem}");
    }
    if report.problems.is_empty() {
        Ok(())
    } else {
        Err(RunError::Failed(format!(
            "sync-apply reported {} problem(s)",
            report.problems.len()
        )))
    }
}
