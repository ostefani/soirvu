// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! open_link and settings (§6.1).

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::oneshot;

use crate::error::ipc;
use crate::platform;
use crate::state::{AppState, Settings, SettingsPatch};

const MAX_OUTBOUND_URL_BYTES: usize = 4096;

fn contains_unsafe_url_text(text: &str) -> bool {
    text.chars().any(|c| c.is_control()
        || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}

fn parse_outbound_url(input: &str) -> Option<tauri::Url> {
    if input.len() > MAX_OUTBOUND_URL_BYTES
        || input.chars().any(char::is_whitespace)
        || input.contains('\\')
        || contains_unsafe_url_text(input)
        || contains_unsafe_url_text(&percent_encoding::percent_decode_str(input).decode_utf8().ok()?)
    {
        return None;
    }
    let url = tauri::Url::parse(input).ok()?;
    let (_, rest) = input.split_once(':')?;
    match url.scheme() {
        "https" => {
            let authority = rest.strip_prefix("//")?.split(['/', '?', '#']).next()?;
            if authority.is_empty() || authority.contains('@') || url.host().is_none() {
                return None;
            }
        }
        // Allow one address and optional subject/body. Adding recipients,
        // attachments or arbitrary mail headers is outside this command.
        "mailto" => {
            if !url.cannot_be_a_base() || url.fragment().is_some() {
                return None;
            }
            let recipient = percent_encoding::percent_decode_str(url.path()).decode_utf8().ok()?;
            let (local, domain) = recipient.split_once('@')?;
            if local.is_empty() || domain.is_empty()
                || !local.bytes().all(|b| b.is_ascii_alphanumeric() || b".!#$&'*+-=_~".contains(&b))
                || !domain.split('.').all(|label| {
                    !label.is_empty() && !label.starts_with('-') && !label.ends_with('-')
                        && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return None;
            }
            let (mut subject, mut body) = (false, false);
            for field in url.query().into_iter().flat_map(|query| query.split('&')) {
                let (key, _) = field.split_once('=')?;
                let seen = match key {
                    "subject" => &mut subject,
                    "body" => &mut body,
                    _ => return None,
                };
                if std::mem::replace(seen, true) {
                    return None;
                }
            }
        }
        _ => return None,
    }
    (url.as_str().len() <= MAX_OUTBOUND_URL_BYTES).then_some(url)
}

#[tauri::command]
pub async fn open_link(app: AppHandle, url: String) -> Result<bool, String> {
    let url = parse_outbound_url(&url).ok_or_else(|| "invalid_input".to_string())?;
    let destination = if url.scheme() == "https" {
        url.origin().ascii_serialization()
    } else {
        url.path().to_owned()
    };
    // A native confirmation, from Rust: the webview cannot skip it.
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(format!("Open this link?\n\nDestination: {destination}\n\n{url}"))
        .title("SoirVu")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Open".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    if !rx.await.unwrap_or(false) {
        return Ok(false);
    }
    ipc(platform::open_external(std::ffi::OsStr::new(url.as_str())).map_err(Into::into))?;
    Ok(true)
}

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> Result<Settings, String> {
    crate::commands::run_blocking_state(app, |state| Ok(state.settings())).await
}

#[tauri::command]
pub async fn settings_set(app: AppHandle, settings: SettingsPatch) -> Result<(), String> {
    let (previous, next) = ipc(app.state::<AppState>().prepare_settings(settings))?;
    if next.weakens_protection(&previous) {
        let (tx, rx) = oneshot::channel();
        app.dialog()
            .message("Allow this change to the diary’s lock, privacy, or attachment limits?")
            .title("Change diary protection settings")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom("Allow change".into(), "Cancel".into()))
            .show(move |confirmed| { let _ = tx.send(confirmed); });
        if !rx.await.unwrap_or(false) {
            return Err("cancelled".into());
        }
    }
    crate::commands::run_blocking_state(app, move |state| ipc(state.commit_settings(previous, next))).await
}

/// Picks the backup folder with the native dialog and stores
/// the PATH in Rust (app.json). The UI gets back the updated
/// settings, whose backup_location is a read-only full path. The user
/// should always know where the backups are.
#[tauri::command]
pub async fn settings_pick_backup_dir(app: AppHandle) -> Result<Option<Settings>, String> {
    let Some(path) = crate::commands::vault::pick_folder(&app).await else {
        return Ok(None);
    };
    let settings = crate::commands::run_blocking_state(app, move |state| {
        ipc(state.set_backup_dir(Some(path)))?;
        Ok(state.settings())
    })
    .await?;
    Ok(Some(settings))
}

#[tauri::command]
pub async fn settings_clear_backup_dir(app: AppHandle) -> Result<Settings, String> {
    crate::commands::run_blocking_state(app, |state| {
        ipc(state.set_backup_dir(None))?;
        Ok(state.settings())
    })
    .await
}

/// §9 (optional, off by default): the UI reports every copy made
/// in the diary; 30 s later the clipboard is cleared, but only
/// if nothing else has been copied since. The clipboard's
/// content is never read — only its change counter.
#[tauri::command]
pub fn clipboard_note_copy(state: State<'_, AppState>) -> Result<(), String> {
    if !state.clipboard_clear() {
        return Ok(());
    }
    let Some(count) = platform::clipboard_change_count() else {
        return Ok(());
    };
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(30));
        platform::clipboard_clear_if_unchanged(count);
    });
    Ok(())
}

/// §13 Phase 6: copy the open vault to the backup location
/// now, whatever the schedule says. Returns the new folder's
/// display name; the path never crosses the boundary.
#[tauri::command]
pub async fn backup_now(app: AppHandle) -> Result<String, String> {
    let session = ipc(app.state::<AppState>().session())?;
    crate::commands::run_blocking_state(app, move |state| ipc(crate::backup::make_backup(state, &session))).await
}
/// What an export wrote (§8): counts only, plus a display
/// name for the destination — paths never cross the IPC
/// boundary.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutcome {
    pub entries: usize,
    pub attachments: usize,
    pub target: String,
}

/// The §8 warning, native so the webview cannot skip it.
/// True = the user chose to export plaintext anyway.
async fn confirm_export(app: &AppHandle) -> bool {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(
            "Export writes your diary as PLAIN files that anyone can read — \
             no passphrase protects them.\n\n\
             Keep the export somewhere private, and delete it when you no longer need it."
                .to_string(),
        )
        .title("SoirVu")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Export plaintext".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    rx.await.unwrap_or(false)
}

/// Pending dialogs expire when the originating session ends. Once a
/// reauthenticated export starts, it finishes before lock can complete.
async fn run_export(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
    json: bool,
) -> Result<Option<ExportOutcome>, String> {
    use zeroize::Zeroizing;

    let pass = Zeroizing::new(passphrase);
    let session = ipc(state.session())?;
    if !confirm_export(&app).await {
        return Ok(None);
    }
    let Some(dir) = crate::commands::vault::pick_folder(&app).await else {
        return Ok(None);
    };
    let permit = super::vault::INTERACTIVE_KDF.try_acquire().map_err(|_| "busy".to_string())?;
    let result = crate::commands::run_blocking_state(app, move |state| {
        let _permit = permit;
        ipc(state.with_session(&session, |vault| {
            vault.verify_passphrase(&pass)?;
            if json {
                let file = dir.join("soirvu-export.json");
                let report = vault.export_json(&file).map_err(|error| match error {
                    soirvu_core::Error::Io(ref io) if io.kind() == std::io::ErrorKind::AlreadyExists =>
                        crate::error::AppError::Code("export_target_exists".into()),
                    error => crate::error::AppError::from(error),
                })?;
                let folder = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Ok(ExportOutcome {
                    entries: report.entries,
                    attachments: report.attachments,
                    target: format!("{folder}/soirvu-export.json"),
                })
            } else {
                let report = vault.export_markdown(&dir)?;
                Ok(ExportOutcome {
                    entries: report.entries,
                    attachments: report.attachments,
                    target: dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                })
            }
        }))
    })
    .await?;
    Ok(Some(result))
}

/// §8: the whole diary as Markdown files with front matter,
/// attachments loose in `assets/`. Plaintext — see the
/// warning in `confirm_export`.
#[tauri::command]
pub async fn export_markdown(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<Option<ExportOutcome>, String> {
    run_export(app, state, passphrase, false).await
}

/// §8: the whole diary as one lossless JSON file,
/// tombstones and ids included, made for re-import.
#[tauri::command]
pub async fn export_json(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<Option<ExportOutcome>, String> {
    run_export(app, state, passphrase, true).await
}

/// What an import did (§8): counts only — paths never cross
/// the boundary here either.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub entries: usize,
    pub attachments: usize,
    pub skipped_entries: usize,
    pub skipped_attachments: usize,
    pub export_only_attachments: usize,
    pub folders: usize,
    pub skipped_folders: usize,
}

impl From<soirvu_core::import::ImportReport> for ImportOutcome {
    fn from(report: soirvu_core::import::ImportReport) -> Self {
        ImportOutcome {
            entries: report.entries,
            attachments: report.attachments,
            skipped_entries: report.skipped_entries,
            skipped_attachments: report.skipped_attachments,
            export_only_attachments: report.export_only_attachments,
            folders: report.folders,
            skipped_folders: report.skipped_folders,
        }
    }
}

/// §8: import a folder of Markdown files (front matter
/// optional) into the open diary. Only adds — see the core
/// import module for the id rules.
#[tauri::command]
pub async fn import_markdown(app: AppHandle) -> Result<Option<ImportOutcome>, String> {
    let session = ipc(app.state::<AppState>().session())?;
    let Some(dir) = crate::commands::vault::pick_folder(&app).await else {
        return Ok(None);
    };
    let report = crate::commands::run_blocking_state(app, move |state| {
        let parsed = ipc(soirvu_core::import::MarkdownImport::read(&dir).map_err(Into::into))?;
        ipc(state.with_session(&session, |vault| Ok(vault.import_parsed_markdown(parsed)?)))
    })
    .await?;
    Ok(Some(report.into()))
}

/// §8: import a SoirVu JSON export into the open diary.
/// Entries and attachments keep their ids; ids already taken
/// are skipped, never overwritten.
#[tauri::command]
pub async fn import_json(app: AppHandle) -> Result<Option<ImportOutcome>, String> {
    let session = ipc(app.state::<AppState>().session())?;
    let Some(file) = crate::commands::vault::pick_file(&app).await else {
        return Ok(None);
    };
    let report = crate::commands::run_blocking_state(app, move |state| {
        let parsed = ipc(soirvu_core::import::JsonImport::read(&file).map_err(Into::into))?;
        ipc(state.with_session(&session, |vault| Ok(vault.import_parsed_json(parsed)?)))
    })
    .await?;
    Ok(Some(report.into()))
}

#[cfg(test)]
mod url_tests {
    use super::*;

    #[test]
    fn outbound_urls_require_an_explicit_destination_and_safe_mail_fields() {
        let https = parse_outbound_url("HTTPS://Example.COM:443/help?q=a%20b").unwrap();
        assert_eq!(https.as_str(), "https://example.com/help?q=a%20b");
        let mail = "mailto:note+diary@example.com?subject=Diary%20note&body=Hello";
        assert_eq!(parse_outbound_url(mail).unwrap().as_str(), mail);

        for invalid in [
            "http://example.com",
            "file:///notes",
            "https:///example.com",
            "https://user@example.com",
            "https://@example.com",
            "https://example.com/\n",
            "https://example.com/%00",
            "https://example.com/%E2%80%AE",
            "mailto:a@example.com,b@example.com",
            "mailto:a@example.com?bcc=b@example.com",
            "mailto:a@example.com?subject=one&subject=two",
            "mailto:a@example.com?body=hello%0D%0Aworld",
            "mailto:a@example.com#fragment",
        ] {
            assert!(parse_outbound_url(invalid).is_none(), "{invalid:?}");
        }
        assert!(parse_outbound_url(&format!("https://example.com/{}", "a".repeat(MAX_OUTBOUND_URL_BYTES))).is_none());
    }
}
