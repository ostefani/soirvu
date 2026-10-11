use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

const KEY: [u8; 32] = [7; 32]; // fake key for the test; the app derives a real one
const WRONG_KEY: [u8; 32] = [8; 32];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Opens or creates the file and unlocks it with a raw 32-byte key.
/// The raw form x'<64 hex digits>' skips SQLCipher's own key derivation.
/// PRAGMA key must be the first statement on the connection.
fn open(path: &Path, key: &[u8; 32]) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", hex(key)))?;
    Ok(conn)
}

/// Returns the titles of entries matching `query`, in insertion order.
/// The query is wrapped in quotes so FTS5 treats it as plain text.
fn search(conn: &Connection, query: &str) -> rusqlite::Result<Vec<String>> {
    let phrase = format!("\"{}\"", query.replace('"', "\"\""));
    let mut stmt = conn.prepare(
        "SELECT e.title FROM entry_fts
         JOIN entry e ON e.rid = entry_fts.rowid
         WHERE entry_fts MATCH ?1
         ORDER BY e.rid",
    )?;
    let titles = stmt
        .query_map([phrase], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(titles)
}

fn temp_db_path() -> PathBuf {
    std::env::temp_dir().join(format!("diary_spike_{}.db", std::process::id()))
}

#[test]
fn sqlcipher_with_trigram_search_for_russian() -> Result<(), Box<dyn std::error::Error>> {
    let path = temp_db_path();
    let _ = std::fs::remove_file(&path);

    // 1. Create an encrypted database and confirm SQLCipher (not plain SQLite) is linked.
    let conn = open(&path, &KEY)?;
    let cipher_version: String = conn.query_row("PRAGMA cipher_version", [], |r| r.get(0))?;
    println!("SQLCipher version: {cipher_version}");
    assert!(!cipher_version.is_empty());

    // 2. Same shape as the design: a content table plus an external-content FTS5 index.
    conn.execute_batch(
        "CREATE TABLE entry (rid INTEGER PRIMARY KEY, title TEXT, body_md TEXT);
         CREATE VIRTUAL TABLE entry_fts USING fts5(
             title, body_md, content='entry', content_rowid='rid', tokenize='trigram');
         CREATE TRIGGER entry_ai AFTER INSERT ON entry BEGIN
             INSERT INTO entry_fts(rowid, title, body_md)
             VALUES (new.rid, new.title, new.body_md);
         END;",
    )?;

    let rows = [
        ("Покупки", "Сегодня купила Книгу о море."),
        ("Вечер", "Читала книги до ночи."),
        ("Список", "Одна книга лежит на столе."),
        ("Кино", "Вечером пошли в кино."),
        ("Лес", "Нашёл ёлку и белку."),
        ("English", "I bought a Book about the sea."),
    ];
    for (title, body) in rows {
        conn.execute(
            "INSERT INTO entry (title, body_md) VALUES (?1, ?2)",
            params![title, body],
        )?;
    }

    // 3. Search behaviour we want for Russian.
    // Lowercase query finds capitalised «Книгу» and the other forms: case-insensitive substring.
    assert_eq!(search(&conn, "книг")?, ["Покупки", "Вечер", "Список"]);
    assert_eq!(search(&conn, "КНИГ")?, ["Покупки", "Вечер", "Список"]);
    // It is substring matching, not stemming: the full form «книгу» matches only that form.
    assert_eq!(search(&conn, "книгу")?, ["Покупки"]);
    // English works the same way.
    assert_eq!(search(&conn, "book")?, ["English"]);
    // Queries under 3 characters match nothing with this tokenizer.
    assert!(search(&conn, "кн")?.is_empty());
    // «ё» and «е» are different letters, as the design assumes.
    assert_eq!(search(&conn, "нашёл")?, ["Лес"]);
    assert!(search(&conn, "нашел")?.is_empty());
    drop(conn);

    // 4. The file on disk is really encrypted.
    let bytes = std::fs::read(&path)?;
    assert!(!bytes.starts_with(b"SQLite format 3"));
    let needle = "книг".as_bytes();
    assert!(!bytes.windows(needle.len()).any(|w| w == needle));

    // 5. The wrong key cannot read it; the right key still can.
    let bad = open(&path, &WRONG_KEY)?; // opening does not check the key yet
    let result = bad.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0));
    assert!(result.is_err());
    let good = open(&path, &KEY)?;
    let count: i64 = good.query_row("SELECT count(*) FROM entry", [], |r| r.get(0))?;
    assert_eq!(count, 6);

    let _ = std::fs::remove_file(&path);
    Ok(())
}
