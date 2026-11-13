//! Public backlink retrieval contracts, exercised with the production core.
use soirvu_core::Vault;
use soirvu_core::crypto::KdfParams;
use soirvu_core::entries::EntryDraft;
use tempfile::TempDir;

fn fast() -> KdfParams {
    KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }
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

fn candidates(vault: &Vault, id: &str) -> Vec<String> {
    let page = vault.backlink_candidates(id, 20, None).unwrap();
    assert!(page.next.is_none());
    page.items.into_iter().map(|item| item.id).collect()
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
fn literal_and_titled_links_are_candidates() {
    let (_tmp, mut vault) = vault();
    let id = target_id(&mut vault);
    let src = vault
        .create_entry(
            None,
            &draft(
                "2026-10-31",
                Some("Summary"),
                &format!("See [notes](entry:{id}) and [notes](entry:{id} \"Training notes\")."),
            ),
        )
        .unwrap();
    assert_eq!(candidates(&vault, &id), vec![src.id]);
}

#[test]
fn reference_definitions_are_candidates() {
    let (_tmp, mut vault) = vault();
    let id = target_id(&mut vault);
    let src = vault
        .create_entry(
            None,
            &draft(
                "2026-10-31",
                Some("Summary"),
                &format!("[notes][t]\n\n[t]: entry:{id}"),
            ),
        )
        .unwrap();
    assert_eq!(candidates(&vault, &id), vec![src.id]);
}

#[test]
fn title_only_mentions_are_not_body_candidates() {
    let (_tmp, mut vault) = vault();
    let id = target_id(&mut vault);
    // The UUID appears only in a title, never in a body.
    vault
        .create_entry(
            None,
            &draft("2026-10-31", Some(&format!("about {id}")), "no links here"),
        )
        .unwrap();
    assert!(candidates(&vault, &id).is_empty());
}

