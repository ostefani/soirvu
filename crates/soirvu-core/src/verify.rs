//! Vault verification: is everything that should be there really there,
//! and does it all still decrypt?
//!
//! Used by `soirvu-cli verify` and meant for a future "check my vault"
//! button. It reads a lot and writes nothing. Problems are collected, not
//! stopped at: a damaged vault should give the full list in one run.

use crate::attachments::blob_path;
use crate::error::Result;
use crate::folder::Vault;

/// The result of checking a vault. `problems` empty means healthy.
/// Problem texts name ids, never diary content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifyReport {
    pub entries: i64,
    pub tombstones: i64,
    pub attachments: i64,
    pub attachment_bytes: i64,
    pub problems: Vec<String>,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

impl Vault {
    /// Checks, in order:
    /// 1. the database itself (`integrity_check`, `cipher_integrity_check`);
    /// 2. the search index against the entry table (FTS5 `integrity-check`);
    /// 3. every live attachment: its blob file exists and `read_attachment`
    ///    verifies every chunk and the stored SHA-256.
    pub fn verify(&self) -> Result<VerifyReport> {
        let conn = self.db.connection();
        let mut report = VerifyReport {
            entries: conn.query_row(
                "SELECT count(*) FROM entry WHERE deleted_at IS NULL",
                [],
                |r| r.get(0),
            )?,
            tombstones: conn.query_row(
                "SELECT count(*) FROM entry WHERE deleted_at IS NOT NULL",
                [],
                |r| r.get(0),
            )?,
            attachments: conn.query_row(
                "SELECT count(*) FROM attachment WHERE deleted_at IS NULL",
                [],
                |r| r.get(0),
            )?,
            attachment_bytes: conn
                .query_row(
                    "SELECT sum(size) FROM attachment WHERE deleted_at IS NULL",
                    [],
                    |r| r.get::<_, Option<i64>>(0),
                )?
                .unwrap_or(0),
            problems: Vec::new(),
        };

        // 1. Database pages.
        for pragma in ["integrity_check", "cipher_integrity_check"] {
            let mut stmt = conn.prepare(&format!("PRAGMA {pragma}"))?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                let text = row?;
                // A healthy database answers "ok" (integrity_check) or
                // nothing at all (cipher_integrity_check). Anything else
                // is a problem line.
                if text != "ok" {
                    report.problems.push(format!("{pragma}: {text}"));
                }
            }
        }

        // 2. Search index.
        if conn
            .execute(
                "INSERT INTO entry_fts(entry_fts, rank) VALUES ('integrity-check', 1)",
                [],
            )
            .is_err()
        {
            report
                .problems
                .push("the search index does not match the entry table".to_string());
        }

        // 3. Attachments.
        let ids: Vec<String> = {
            let mut stmt =
                conn.prepare("SELECT id FROM attachment WHERE deleted_at IS NULL ORDER BY id")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in &ids {
            if !blob_path(self.dir(), id).exists() {
                report
                    .problems
                    .push(format!("attachment {id}: blob file is missing"));
            } else if let Err(e) = self.read_attachment(id) {
                report.problems.push(e.to_string());
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;
    use crate::entries::EntryDraft;
    use std::fs;
    use tempfile::TempDir;

    fn fast() -> KdfParams {
        KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        }
    }

    fn vault_with_attachment() -> (TempDir, Vault, String) {
        let tmp = TempDir::new().unwrap();
        let (mut vault, _recovery) =
            Vault::create(&tmp.path().join("v"), "pass phrase", fast()).unwrap();
        let entry = vault
            .create_entry(&EntryDraft {
                entry_date: "2026-10-13".into(),
                title: Some("День".into()),
                body_md: "про книгу".into(),
                ..Default::default()
            })
            .unwrap();
        let att = vault
            .add_attachment(&entry.id, "a.bin", "application/octet-stream", b"content")
            .unwrap();
        (tmp, vault, att.id)
    }

    #[test]
    fn a_healthy_vault_verifies_clean() {
        let (_tmp, mut vault, _att_id) = vault_with_attachment();
        let gone = vault
            .create_entry(&EntryDraft {
                entry_date: "2026-10-12".into(),
                body_md: "gone".into(),
                ..Default::default()
            })
            .unwrap();
        vault.delete_entry(&gone.id).unwrap();

        let report = vault.verify().unwrap();
        assert!(report.ok(), "problems: {:?}", report.problems);
        assert_eq!(report.entries, 1);
        assert_eq!(report.tombstones, 1);
        assert_eq!(report.attachments, 1);
        assert_eq!(report.attachment_bytes, 7);
    }

    #[test]
    fn a_tampered_blob_is_reported_not_hidden() {
        let (_tmp, vault, att_id) = vault_with_attachment();
        let path = blob_path(vault.dir(), &att_id);
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(&path, bytes).unwrap();

        let report = vault.verify().unwrap();
        assert!(!report.ok());
        assert_eq!(report.problems.len(), 1);
        assert!(report.problems[0].contains(&att_id), "{:?}", report.problems);
        // The counts are still reported alongside the problem.
        assert_eq!(report.attachments, 1);
    }

    #[test]
    fn a_missing_blob_is_reported() {
        let (_tmp, vault, att_id) = vault_with_attachment();
        fs::remove_file(blob_path(vault.dir(), &att_id)).unwrap();

        let report = vault.verify().unwrap();
        assert!(!report.ok());
        assert_eq!(report.problems.len(), 1);
        assert!(report.problems[0].contains("missing"), "{:?}", report.problems);
    }

    #[test]
    fn an_empty_vault_verifies_clean() {
        let tmp = TempDir::new().unwrap();
        let (vault, _recovery) = Vault::create(&tmp.path().join("v"), "pass phrase", fast()).unwrap();
        let report = vault.verify().unwrap();
        assert!(report.ok(), "problems: {:?}", report.problems);
        assert_eq!((report.entries, report.attachments), (0, 0));
    }
}
