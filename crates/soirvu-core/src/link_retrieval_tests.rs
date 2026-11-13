// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Step-1 retrieval fixtures for folders and entry links.
//!
//! These prove the retrieval contracts the design depends on, before any
//! schema or API changes: body-only UUID candidate lookup, title-only
//! exclusion, the raw-FTS false positives that JavaScript verification
//! must filter, and the sub-3-character query limits that force the
//! `title_key` prefix tier. All assertions pass on the current code.

use crate::Vault;
use crate::crypto::KdfParams;
use crate::entries::EntryDraft;
use tempfile::TempDir;

fn fast() -> KdfParams {
    KdfParams {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    }
}

fn vault() -> (TempDir, Vault) {
    let tmp = TempDir::new().unwrap();
    let (vault, _recovery) = Vault::create(&tmp.path().join("v"), "pass phrase", fast()).unwrap();
    (tmp, vault)
}

fn draft(date: &str, title: Option<&str>, body: &str) -> EntryDraft {
    EntryDraft {
        entry_date: date.into(),
        title: title.map(str::to_string),
        body_md: body.into(),
        ..Default::default()
    }
}

/// Body-only UUID candidate lookup, as specified for backlinks:
/// quoted UUID, `body_md` column filter, live entries only.
fn candidates(vault: &Vault, uuid: &str) -> Vec<String> {
    let phrase = format!("body_md : \"{uuid}\"");
    let mut stmt = vault
        .database()
        .connection()
        .prepare(
            "SELECT e.id FROM entry_fts JOIN entry e ON e.rid = entry_fts.rowid
              WHERE entry_fts MATCH ?1 AND e.deleted_at IS NULL
              ORDER BY e.entry_date DESC, e.created_at DESC, e.id DESC",
        )
        .unwrap();
    stmt.query_map([phrase], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

/// The target both link forms point at.
fn target_id(vault: &mut Vault) -> String {
    vault
        .create_entry(
            None,
            &draft("2026-10-31", Some("Training notes"), "legs day"),
        )
        .unwrap()
        .id
}

#[test]
fn raw_fts_also_matches_what_verification_must_reject() {
    let (_tmp, mut vault) = vault();
    let id = target_id(&mut vault);
    // Code spans, images and invalid destinations all match a raw substring
    // lookup today. JavaScript verification (not the query) must reject
    // them; this test pins the raw behavior so a future query change that
    // silently drops one of these rows is noticed.
    let code = vault
        .create_entry(
            None,
            &draft("2026-10-31", Some("Example"), &format!("`[x](entry:{id})`")),
        )
        .unwrap();
    let image = vault
        .create_entry(
            None,
            &draft("2026-10-31", Some("Photo"), &format!("![x](entry:{id})")),
        )
        .unwrap();
    let mut found = candidates(&vault, &id);
    found.sort();
    let mut want = vec![code.id.clone(), image.id.clone()];
    want.sort();
    assert_eq!(found, want);
    // An invalid destination (extra characters) matches raw FTS too:
    // all three rows are candidates, and JavaScript verification must
    // reject all three.
    let typo = vault
        .create_entry(
            None,
            &draft("2026-10-31", Some("Typo"), &format!("[x](entry:{id}extra)")),
        )
        .unwrap();
    let mut found = candidates(&vault, &id);
    found.sort();
    let mut want = vec![code.id, image.id, typo.id];
    want.sort();
    assert_eq!(found, want);
}

#[test]
fn deleted_sources_leave_no_candidates() {
    let (_tmp, mut vault) = vault();
    let id = target_id(&mut vault);
    let src = vault
        .create_entry(
            None,
            &draft(
                "2026-10-31",
                Some("Summary"),
                &format!("[notes](entry:{id})"),
            ),
        )
        .unwrap();
    assert_eq!(candidates(&vault, &id), vec![src.id.clone()]);
    vault.delete_entry(&src.id).unwrap();
    assert!(candidates(&vault, &id).is_empty());
}

#[test]
fn sub_three_character_match_returns_nothing() {
    // Locks the premise behind the title_key prefix tier: trigram FTS
    // cannot answer 1–2 character queries, in any script or form.
    let (_tmp, mut vault) = vault();
    vault
        .create_entry(None, &draft("2026-10-31", Some("Training notes"), "body"))
        .unwrap();
    vault
        .create_entry(None, &draft("2026-10-31", Some("Молитва дня"), "body"))
        .unwrap();
    for q in ["Tr", "T", "Мо", "мо"] {
        let n: i64 = vault
            .database()
            .connection()
            .query_row(
                "SELECT count(*) FROM entry_fts WHERE entry_fts MATCH ?1",
                [format!("title : \"{q}\"")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "query {q:?} unexpectedly matched");
    }
    // ...and neither does the wildcard form.
    let n: i64 = vault
        .database()
        .connection()
        .query_row(
            "SELECT count(*) FROM entry_fts WHERE entry_fts MATCH ?1",
            ["title : \"Tr\"*"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn ascii_prefix_like_retrieves_while_sqlite_lower_stops_at_ascii() {
    let (_tmp, mut vault) = vault();
    vault
        .create_entry(None, &draft("2026-10-31", Some("Training notes"), "body"))
        .unwrap();
    vault
        .create_entry(None, &draft("2026-10-31", Some("Молитва дня"), "body"))
        .unwrap();
    let ascii: Vec<String> = vault
        .database()
        .connection()
        .prepare("SELECT title FROM entry WHERE title LIKE ?1 ESCAPE '\\'")
        .unwrap()
        .query_map(["Tr%"], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(ascii, vec!["Training notes".to_string()]);
    // SQLite lower() folds ASCII only: a Cyrillic case-insensitive prefix
    // needs the application normalizer (the future title_key), not SQL.
    let n: i64 = vault
        .database()
        .connection()
        .query_row(
            "SELECT count(*) FROM entry WHERE lower(title) LIKE lower(?)",
            ["молитва%"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}
