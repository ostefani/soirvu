//! Scheduled and manual vault backups (§13 Phase 6).
//!
//! A backup is a plain copy of the open vault's two files — the
//! database (after a WAL checkpoint, so the copy is whole) and
//! the key-slot header — into the folder the user picked in
//! Phase 3, under `<vault folder>-<id8>/<timestamp>/`. The copy
//! stays encrypted: restoring means pointing the app at the
//! copied folder and unlocking it as usual.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};
use crate::state::{AppState, BackupSchedule};

/// How many backups to keep per vault.
const KEEP: usize = 7;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
static SCHEDULED_BACKUP_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn unavailable() -> AppError {
    AppError::Code("backup_unavailable".into())
}

/// Whether a backup is due: never when off, always when none
/// exists yet, otherwise once the period has passed.
pub fn backup_due(schedule: BackupSchedule, last: Option<SystemTime>, now: SystemTime) -> bool {
    let period = match schedule {
        BackupSchedule::Off => return false,
        BackupSchedule::Daily => DAY,
        BackupSchedule::Weekly => DAY * 7,
    };
    match last {
        None => true,
        Some(t) => now.duration_since(t).map(|d| d >= period).unwrap_or(false),
    }
}

/// `2026-10-15_032007` in UTC — sorts like time and is safe as a
/// folder name on every filesystem.
pub fn stamp_name(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since the Unix epoch -> (year, month, day), UTC
/// (Howard Hinnant's civil-from-days algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The stamp shape this module writes; other folders in the
/// backup location are never counted or pruned.
pub(crate) fn is_stamp(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 17
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'_',
            _ => c.is_ascii_digit(),
        })
}

/// The per-vault folder inside the backup location.
fn per_vault_dir(backup_root: &Path, src: &Path, vault_id: &str) -> PathBuf {
    let folder = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "vault".into());
    let short: String = vault_id.chars().take(8).collect();
    backup_root.join(format!("{folder}-{short}"))
}

/// The newest backup's creation time in a per-vault folder.
fn last_backup(dir: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_stamp(&name) {
            continue;
        }
        if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
            newest = Some(newest.map_or(mtime, |n: SystemTime| n.max(mtime)));
        }
    }
    newest
}

/// Deletes all but the newest KEEP backup folders. The folder
/// the live vault sits in (and anything containing it) is
/// never deleted: a vault restored into a backup folder by an
/// older version must not be pruned from under itself.
fn prune(dir: &Path, keep_path: &Path) {
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut stamps: Vec<PathBuf> = read
        .flatten()
        .filter(|e| is_stamp(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    stamps.sort();
    if stamps.len() > KEEP {
        for old in &stamps[..stamps.len() - KEEP] {
            if keep_path == old || keep_path.starts_with(old) {
                continue;
            }
            let _ = fs::remove_dir_all(old);
        }
    }
}

/// Copies the open vault into the backup location and returns
/// the new folder's display name (`<vault>-<id8>/<stamp>`).
pub fn make_backup(state: &AppState) -> AppResult<String> {
    let Some(root) = state.backup_dir() else {
        return Err(unavailable());
    };
    if !root.is_dir() {
        return Err(unavailable());
    }
    let id = state
        .current_id()
        .ok_or_else(|| AppError::Code("locked".into()))?;
    let src = state.vault_path(&id)?;
    let per_vault = per_vault_dir(&root, &src, &id);
    state.with_vault_exclusive(|vault| {
        if vault.header().vault_id != id {
            return Err(AppError::Code("locked".into()));
        }
        // Checkpoint and copy form one operation: no write or key change can
        // slip between them and produce a mixed-time backup.
        vault.checkpoint()?;
        fs::create_dir_all(&per_vault).map_err(|_| unavailable())?;

        let stamp = stamp_name(SystemTime::now());
        let tmp = per_vault.join(format!(".{stamp}.tmp"));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).map_err(|_| unavailable())?;
        let copied = (|| -> std::io::Result<()> {
            fs::copy(src.join("soirvu.db"), tmp.join("soirvu.db"))?;
            fs::copy(src.join("vault.header.json"), tmp.join("vault.header.json"))?;
            Ok(())
        })();
        if copied.is_err() {
            let _ = fs::remove_dir_all(&tmp);
            return Err(unavailable());
        }
        let dest = per_vault.join(&stamp);
        let _ = fs::remove_dir_all(&dest);
        crate::platform::rename_retry(&tmp, &dest).map_err(|_| unavailable())?;
        prune(&per_vault, &src);

        let display = per_vault
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(format!("{display}/{stamp}"))
    })
}

/// The watcher's scheduled pass: backs up the open vault when
/// the rule says one is due. Every failure is silent — the
/// volume may simply be unmounted; the next pass retries.
pub fn maybe_scheduled(state: &AppState) {
    use std::sync::atomic::Ordering;
    if SCHEDULED_BACKUP_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            SCHEDULED_BACKUP_RUNNING.store(false, std::sync::atomic::Ordering::Release);
        }
    }
    let _reset = Reset;
    let schedule = state.backup_schedule();
    if schedule == BackupSchedule::Off {
        return;
    }
    let (Some(root), Some(id)) = (state.backup_dir(), state.current_id()) else {
        return;
    };
    let Ok(src) = state.vault_path(&id) else {
        return;
    };
    let dir = per_vault_dir(&root, &src, &id);
    if backup_due(schedule, last_backup(&dir), SystemTime::now()) {
        let _ = make_backup(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_utc_and_sortable() {
        assert_eq!(stamp_name(UNIX_EPOCH), "1970-01-01_000000");
        assert_eq!(
            stamp_name(UNIX_EPOCH + Duration::from_secs(1_792_034_407)),
            "2026-10-15_032007"
        );
        assert!(is_stamp("2026-10-15_032007"));
        assert!(!is_stamp("2026-10-15_032007.tmp"));
        assert!(!is_stamp("notes"));
    }

    #[test]
    fn due_rules() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        assert!(!backup_due(BackupSchedule::Off, None, now));
        assert!(backup_due(BackupSchedule::Daily, None, now));
        assert!(!backup_due(BackupSchedule::Daily, Some(now - DAY / 2), now));
        assert!(backup_due(BackupSchedule::Daily, Some(now - DAY), now));
        assert!(!backup_due(
            BackupSchedule::Weekly,
            Some(now - DAY * 3),
            now
        ));
        assert!(backup_due(BackupSchedule::Weekly, Some(now - DAY * 7), now));
    }

    #[test]
    fn prune_never_deletes_the_live_vaults_folder() {
        let tmp = tempfile::TempDir::new().unwrap();
        for i in 0..9 {
            fs::create_dir(tmp.path().join(format!("2026-10-{i:02}_000000"))).unwrap();
        }
        let live = tmp.path().join("2026-10-00_000000");
        prune(tmp.path(), &live);
        assert!(live.exists(), "the live vault's folder survives pruning");
    }

    #[test]
    fn prune_keeps_the_newest_seven() {
        let tmp = tempfile::TempDir::new().unwrap();
        for i in 0..9 {
            fs::create_dir(tmp.path().join(format!("2026-10-{i:02}_000000"))).unwrap();
        }
        fs::create_dir(tmp.path().join("unrelated")).unwrap();
        prune(tmp.path(), tmp.path());
        let mut names: Vec<String> = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 8); // 7 stamps + the unrelated folder
        assert!(!tmp.path().join("2026-10-00_000000").exists());
        assert!(!tmp.path().join("2026-10-01_000000").exists());
        assert!(tmp.path().join("2026-10-08_000000").exists());
        assert!(tmp.path().join("unrelated").exists());
    }
}
