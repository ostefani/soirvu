// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Attachment commands and the soirvu-asset:// protocol (§6.1, §6.4).
//!
//! Uploads arrive in slices through attachment_chunk (base64 — the
//! isolation pattern carries JSON, not binary bodies), are sniffed
//! and capped in Rust at attachment_finish, have EXIF stripped when
//! the setting says so, and are committed by the core. The UI only
//! ever holds ids.

use std::io::Read;

use serde::Serialize;
use soirvu_core::attachments::{sniff_mime, validate_attachment_id};
use tauri::{AppHandle, Manager};
use zeroize::Zeroize;

use crate::error::{AppError, ipc};
use crate::state::{AppState, VaultSession};

/// The entry's post-commit revision rides along: the editor
/// saves against the rev it opened, so without it every save
/// after an attachment would fail with `revision_conflict`.
#[derive(Debug, Serialize)]
pub struct AttachmentAdded {
    pub id: String,
    pub mime: String,
    pub size: i64,
    pub rev: i64,
}

#[derive(Debug, Serialize)]
pub struct AttachmentInfo {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub previewable: bool,
}

/// The size cap (bytes) for a sniffed type, from the settings (§6.4:
/// caps are settings). None when the type is outside the allowlist.
fn cap_for(mime: &str, caps: &crate::state::Caps) -> Option<u64> {
    let mb = |v: u64| v.saturating_mul(1024 * 1024);
    match mime {
        m if m.starts_with("image/") => Some(mb(caps.image_mb)),
        "application/pdf" => Some(mb(caps.pdf_mb)),
        m if m.starts_with("audio/") => Some(mb(caps.audio_mb)),
        m if m.starts_with("video/") => Some(mb(caps.video_mb)),
        _ => None,
    }
}

fn preview_cap(mime: &str, caps: &crate::state::Caps) -> Option<u64> {
    if mime == "application/pdf" {
        return None;
    }
    cap_for(mime, caps).map(|cap| cap.min(MAX_UPLOAD_HARD_CAP as u64))
}

const MAX_UPLOAD_CHUNK_BYTES: usize = 1024 * 1024;
const MAX_UPLOAD_BASE64_BYTES: usize = MAX_UPLOAD_CHUNK_BYTES.div_ceil(3) * 4;
const MAX_ASSET_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
/// Hard ceiling for one upload, independent of settings.
pub const MAX_UPLOAD_HARD_CAP: usize = 1024 * 1024 * 1024;
/// Uploads remain in memory until finish. Bound both aggregate memory and
/// the number of outstanding upload records, even when IPC calls overlap.
pub const MAX_UPLOAD_TOTAL: usize = 1024 * 1024 * 1024;
pub const MAX_ACTIVE_UPLOADS: usize = 8;

// ---------------------------------------------------------------------------
// Chunked upload (§6.1)

#[tauri::command]
pub async fn attachment_begin(
    app: AppHandle,
    entry_id: String,
    name: String,
    mime_hint: Option<String>,
) -> Result<String, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.begin_upload(entry_id, name, mime_hint))
    })
    .await
}

#[tauri::command]
pub async fn attachment_chunk(
    app: AppHandle,
    upload_id: String,
    data_base64: String,
) -> Result<(), String> {
    let data_base64 = zeroize::Zeroizing::new(data_base64);
    if data_base64.len() > MAX_UPLOAD_BASE64_BYTES {
        return Err("too_large".into());
    }
    static CHUNK_REQUESTS: tokio::sync::Semaphore =
        tokio::sync::Semaphore::const_new(MAX_ACTIVE_UPLOADS);
    let permit = CHUNK_REQUESTS.try_acquire().map_err(|_| "too_many_uploads".to_string())?;
    crate::commands::run_blocking_state(app, move |state| {
        let _permit = permit;
        let mut bytes = decode_base64(&data_base64).map_err(|()| "invalid_input".to_string())?;
        let result = if bytes.len() > MAX_UPLOAD_CHUNK_BYTES {
            Err("too_large".into())
        } else {
            ipc(state.push_chunk(&upload_id, &bytes))
        };
        use zeroize::Zeroize;
        bytes.zeroize();
        result
    })
    .await
}

#[tauri::command]
pub async fn attachment_finish(
    app: AppHandle,
    upload_id: String,
    expected_rev: i64,
) -> Result<AttachmentAdded, String> {
    let session = ipc(app.state::<AppState>().session())?;
    crate::commands::run_blocking_state(app, move |state| {
        let mut upload = ipc(state.take_upload(&upload_id))?;
        let bytes = std::mem::take(&mut upload.bytes);
        ipc(finish_upload(
            state,
            &session,
            upload.entry_id.clone(),
            upload.name.clone(),
            bytes,
            expected_rev,
        ))
    })
    .await
}

#[tauri::command]
pub async fn attachment_abort(app: AppHandle, upload_id: String) {
    let _ = crate::commands::run_blocking_state(app, move |state| {
        state.drop_upload(&upload_id);
        Ok(())
    })
    .await;
}

/// The editor saves against the rev it opened: if sync (or any
/// other writer) advanced the entry while the file uploaded,
/// committing here would strand the attachment on unseen content
/// and the rev adoption would clobber it. Refuse with the same
/// conflict the next save would hit instead.
fn check_base(
    vault: &mut soirvu_core::Vault,
    entry_id: &str,
    expected_rev: i64,
) -> Result<(), AppError> {
    if vault.get_entry(entry_id)?.rev != expected_rev {
        return Err(AppError::from(soirvu_core::Error::RevisionConflict));
    }
    Ok(())
}

/// Sniff, cap, strip, commit — shared by the chunked upload and the
/// file import.
fn finish_upload(
    state: &AppState,
    session: &VaultSession,
    entry_id: String,
    name: String,
    bytes: Vec<u8>,
    expected_rev: i64,
) -> Result<AttachmentAdded, AppError> {
    let mut bytes = zeroize::Zeroizing::new(bytes);
    let settings = state.settings();
    let mime = sniff_mime(&bytes).ok_or_else(|| AppError::Code("unsupported_type".into()))?;
    let cap =
        cap_for(mime, &settings.caps).ok_or_else(|| AppError::Code("unsupported_type".into()))?;
    if bytes.len() as u64 > cap {
        return Err(AppError::Code("too_large".into()));
    }
    if settings.strip_exif {
        let stripped = strip_metadata(&bytes, mime);
        bytes.zeroize();
        bytes = zeroize::Zeroizing::new(stripped);
    }
    let (added, rev) = state.with_session(session, |vault| {
        check_base(vault, &entry_id, expected_rev)?;
        let a = vault.add_attachment(&entry_id, &name, mime, &bytes)?;
        let rev = vault.get_entry(&entry_id)?.rev;
        Ok((a, rev))
    })?;
    Ok(AttachmentAdded {
        id: added.id,
        mime: added.mime,
        size: added.size,
        rev,
    })
}

// ---------------------------------------------------------------------------
// Info, import, save a copy

#[tauri::command]
pub async fn attachment_info(app: AppHandle, id: String) -> Result<AttachmentInfo, String> {
    crate::commands::run_blocking_state(app, move |state| {
        let caps = state.settings().caps;
        ipc(state.with_vault(|vault| {
            let a = vault.get_attachment(&id)?;
            let mime = match sniff_stored_attachment(vault, &id, a.size) {
                Ok(mime) => mime,
                Err(AppError::Code(code)) if code == "unsupported_type" => {
                    "application/octet-stream"
                }
                Err(error) => return Err(error),
            }
            .to_owned();
            let previewable = preview_cap(&mime, &caps).is_some_and(|cap| a.size as u64 <= cap);
            Ok(AttachmentInfo {
                id: a.id,
                name: a.name,
                mime,
                size: a.size,
                previewable,
            })
        }))
    })
    .await
}

/// Optional §6.1 path for very large files: Rust opens the picker
/// and streams the file into the store; the bytes never pass through
/// the webview.
#[tauri::command]
pub async fn attachment_import(
    app: AppHandle,
    entry_id: String,
    expected_rev: i64,
) -> Result<Option<AttachmentAdded>, String> {
    let session = ipc(app.state::<AppState>().session())?;
    let Some(path) = pick_file(&app).await else {
        return Ok(None);
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    crate::commands::run_blocking_state(app, move |state| {
        ipc(import_path(state, &session, &entry_id, &path, &name, expected_rev).map(Some))
    })
    .await
}

fn import_path(
    state: &AppState,
    session: &VaultSession,
    entry_id: &str,
    path: &std::path::Path,
    name: &str,
    expected_rev: i64,
) -> Result<AttachmentAdded, AppError> {
    let settings = state.settings();
    let too_large = || AppError::Code("too_large".into());
    let io_error = |_| AppError::Code("io".into());
    if !std::fs::metadata(path).map_err(io_error)?.is_file() {
        return Err(AppError::Code("io".into()));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut file = options.open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(AppError::Code("io".into()));
    }
    let total = metadata.len();
    let mut head = [0u8; 8192];
    let head_len = read_up_to(&mut file, &mut head)?;
    let mime = sniff_mime(&head[..head_len])
        .ok_or_else(|| AppError::Code("unsupported_type".into()))?;
    let cap = cap_for(mime, &settings.caps)
        .ok_or_else(|| AppError::Code("unsupported_type".into()))?
        .min(MAX_UPLOAD_HARD_CAP as u64);
    if total > cap || head_len as u64 > cap {
        return Err(too_large());
    }
    let strip = settings.strip_exif && matches!(mime, "image/jpeg" | "image/png" | "image/webp");
    if strip {
        // Strippable kinds are images: their cap (25 MB default)
        // keeps the in-memory pass reasonable.
        let mut bytes = zeroize::Zeroizing::new(Vec::with_capacity(total as usize));
        bytes.extend_from_slice(&head[..head_len]);
        file.take(cap - head_len as u64 + 1).read_to_end(&mut bytes)
            .map_err(|_| AppError::Code("io".into()))?;
        if bytes.len() as u64 > cap {
            return Err(too_large());
        }
        let stripped = strip_metadata(&bytes, mime);
        bytes.zeroize();
        let bytes = zeroize::Zeroizing::new(stripped);
        let (added, rev) = state.with_session(session, |vault| {
            check_base(vault, entry_id, expected_rev)?;
            let a = vault.add_attachment(entry_id, name, mime, &bytes)?;
            let rev = vault.get_entry(entry_id)?.rev;
            Ok((a, rev))
        })?;
        return Ok(AttachmentAdded {
            id: added.id,
            mime: added.mime,
            size: added.size,
            rev,
        });
    }
    let mut source = std::io::Cursor::new(&head[..head_len]).chain(file);
    let mut reader = BoundedAttachmentReader { source: &mut source, remaining: cap };
    let (added, rev) = state.with_session(session, |vault| {
        check_base(vault, entry_id, expected_rev)?;
        let a = vault.add_attachment_from_reader(entry_id, name, mime, &mut reader)
            .map_err(|error| match error {
                soirvu_core::Error::Io(ref io) if io.kind() == std::io::ErrorKind::FileTooLarge => too_large(),
                error => AppError::from(error),
            })?;
        let rev = vault.get_entry(entry_id)?.rev;
        Ok((a, rev))
    })?;
    Ok(AttachmentAdded {
        id: added.id,
        mime: added.mime,
        size: added.size,
        rev,
    })
}

struct BoundedAttachmentReader<R> {
    source: R,
    remaining: u64,
}

impl<R: Read> Read for BoundedAttachmentReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return match self.source.read(&mut [0])? {
                0 => Ok(0),
                _ => Err(std::io::ErrorKind::FileTooLarge.into()),
            };
        }
        let limit = self.remaining.min(buf.len() as u64) as usize;
        let count = self.source.read(&mut buf[..limit])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

fn read_up_to(file: &mut std::fs::File, buf: &mut [u8]) -> Result<usize, AppError> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(m) => n += m,
            Err(_) => return Err(AppError::Code("io".into())),
        }
    }
    Ok(n)
}

async fn pick_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_file(move |choice| {
        let _ = tx.send(choice.and_then(|p| p.into_path().ok()));
    });
    rx.await.ok().flatten()
}

/// Opens an attachment in the OS default app: the decrypted
/// bytes are written to the app-managed opened folder (wiped on
/// lock) under a generated name with a content-derived extension.
/// The webview never sees the path.
#[tauri::command]
pub async fn attachment_open(app: AppHandle, id: String) -> Result<bool, String> {
    if validate_attachment_id(&id).is_err() {
        return Err("invalid_input".into());
    }
    let session = ipc(app.state::<AppState>().session())?;
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_session(&session, |vault| {
            let path = materialize_open_copy_locked(vault, state, &id)?;
            if crate::platform::open_external(&path.to_string_lossy()).is_err() {
                let _ = std::fs::remove_file(path);
                return Ok(false);
            }
            Ok(true)
        }))
    })
    .await
}

fn materialize_open_copy_locked(
    vault: &mut soirvu_core::Vault,
    state: &AppState,
    id: &str,
) -> Result<std::path::PathBuf, AppError> {
    use std::io::Write;

    let info = vault.get_attachment(id)?;
    let size = info.size as u64;
    let mime = sniff_stored_attachment(vault, id, info.size)?;
    let suffix = match mime {
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "image/heic" => ".heic",
        "audio/mpeg" => ".mp3",
        "audio/wav" => ".wav",
        "audio/x-m4a" => ".m4a",
        "video/mp4" => ".mp4",
        "video/3gpp" => ".3gp",
        "video/3gpp2" => ".3g2",
        "video/quicktime" => ".mov",
        "application/pdf" => ".pdf",
        _ => return Err(AppError::Code("unsupported_type".into())),
    };
    let dir = state.opened_dir();
    std::fs::create_dir_all(&dir).map_err(|_| AppError::Code("io".into()))?;
    let mut out = tempfile::Builder::new()
        .prefix("attachment-")
        .suffix(suffix)
        .tempfile_in(dir)
        .map_err(|_| AppError::Code("io".into()))?;
    let mut offset = 0;
    while offset < size {
        let end = (offset + 1024 * 1024).min(size);
        let bytes = zeroize::Zeroizing::new(vault.read_attachment_range(id, offset..end)?);
        out.write_all(&bytes)
            .map_err(|_| AppError::Code("io".into()))?;
        offset = end;
    }
    out.as_file()
        .sync_all()
        .map_err(|_| AppError::Code("io".into()))?;
    let (_, path) = out.keep().map_err(|_| AppError::Code("io".into()))?;
    Ok(path)
}

fn sniff_stored_attachment(
    vault: &soirvu_core::Vault,
    id: &str,
    size: i64,
) -> Result<&'static str, AppError> {
    if size <= 0 {
        return Err(AppError::Code("unsupported_type".into()));
    }
    let header = zeroize::Zeroizing::new(vault.read_attachment_range(id, 0..12)?);
    sniff_mime(&header).ok_or_else(|| AppError::Code("unsupported_type".into()))
}

// ---------------------------------------------------------------------------
// soirvu-asset:// protocol (§6.4)

/// One protocol answer: status, headers, body.
pub struct AssetResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

fn asset(status: u16, mime: &str, body: Vec<u8>, extra: &[(&str, String)]) -> AssetResponse {
    let mut headers = vec![
        ("Content-Type".to_string(), mime.to_string()),
        ("Content-Length".to_string(), body.len().to_string()),
        ("Accept-Ranges".to_string(), "bytes".to_string()),
        ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
        ("Cache-Control".to_string(), "no-store".to_string()),
        (
            "Content-Security-Policy".to_string(),
            "default-src 'none'; sandbox".to_string(),
        ),
    ];
    for (k, v) in extra {
        headers.push((k.to_string(), v.clone()));
    }
    AssetResponse {
        status,
        headers,
        body,
    }
}

/// Serves one attachment by id. Locked vault → 403; unknown or
/// tombstoned id → 404; a satisfiable Range → 206 with exactly those
/// bytes (§6.4: only the needed chunks are decrypted).
pub(crate) fn serve_asset(state: &AppState, session: &VaultSession, id: &str, range_header: Option<&str>) -> AssetResponse {
    let text = |status: u16, msg: &str| asset(status, "text/plain", msg.as_bytes().to_vec(), &[]);
    let caps = state.settings().caps;
    let response = state.with_session(session, |vault| {
        let info = vault.get_attachment(id)?;
        let mime = sniff_stored_attachment(vault, id, info.size)?;
        let total = info.size as u64;
        let cap =
            preview_cap(mime, &caps).ok_or_else(|| AppError::Code("unsupported_type".into()))?;
        if total > cap {
            return Err(AppError::Code("too_large".into()));
        }
        match range_header.and_then(|header| parse_range(header, total)) {
            Some((start, end)) if start > end => Ok(asset(
                416,
                "text/plain",
                Vec::new(),
                &[("Content-Range", format!("bytes */{total}"))],
            )),
            Some((start, end)) => {
                let end = end.min(start + MAX_ASSET_RESPONSE_BYTES - 1);
                Ok(asset(
                    206,
                    mime,
                    vault.read_attachment_range(id, start..end + 1)?,
                    &[("Content-Range", format!("bytes {start}-{end}/{total}"))],
                ))
            }
            None if total > MAX_ASSET_RESPONSE_BYTES => Err(AppError::Code("too_large".into())),
            None => Ok(asset(
                200,
                mime,
                vault.read_attachment_range(id, 0..total)?,
                &[],
            )),
        }
    });
    match response {
        Ok(response) => response,
        Err(AppError::Code(code)) if code == "locked" => text(403, "locked"),
        Err(AppError::Code(code)) if code == "unsupported_type" => text(415, "unsupported type"),
        Err(AppError::Code(code)) if code == "too_large" => {
            text(413, "preview exceeds attachment size limit")
        }
        Err(_) => text(404, "not found"),
    }
}

/// Parses a single `bytes=start-end` range against the body size
/// (suffix and open-ended forms included), clamped to the last
/// byte. Missing or malformed ranges use the bounded full-response path.
fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = header.strip_prefix("bytes=")?.split(',').next()?;
    let (a, b) = spec.split_once('-')?;
    let last = total - 1;
    if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        if n == 0 {
            return None;
        }
        return Some((total.saturating_sub(n), last));
    }
    let start: u64 = a.parse().ok()?;
    let end: u64 = if b.is_empty() { last } else { b.parse().ok()? };
    Some((start, end.min(last)))
}

// ---------------------------------------------------------------------------
// Metadata stripping (§6.4: EXIF/GPS out of JPEG, PNG and WebP)

/// Removes EXIF/XMP metadata segments. Returns the input unchanged
/// for other types or when the structure is not understood — a
/// diary must never corrupt an image over a metadata edge case.
pub fn strip_metadata(bytes: &[u8], mime: &str) -> Vec<u8> {
    match mime {
        "image/jpeg" => strip_jpeg(bytes).unwrap_or_else(|| bytes.to_vec()),
        "image/png" => strip_png(bytes).unwrap_or_else(|| bytes.to_vec()),
        "image/webp" => strip_webp(bytes).unwrap_or_else(|| bytes.to_vec()),
        _ => bytes.to_vec(),
    }
}

fn strip_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 4 || &bytes[0..2] != b"\xFF\xD8" {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[0..2]);
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        if marker == 0xDA {
            // Start of scan: everything from here is image data.
            out.extend_from_slice(&bytes[i..]);
            return Some(out);
        }
        if marker == 0xD9 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            out.extend_from_slice(&bytes[i..i + 2]);
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            return None;
        }
        if marker != 0xE1 {
            // APP1 (0xE1) carries Exif and XMP: dropped. Everything
            // else (including the ICC profile in APP2) is kept.
            out.extend_from_slice(&bytes[i..i + 2 + len]);
        }
        i += 2 + len;
    }
    Some(out)
}

fn strip_png(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1A\n" {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[0..8]);
    let mut i = 8;
    while i + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let kind = &bytes[i + 4..i + 8];
        let total = 12 + len;
        if i + total > bytes.len() {
            return None;
        }
        if kind != b"eXIf" {
            out.extend_from_slice(&bytes[i..i + total]);
        }
        i += total;
        if kind == b"IEND" {
            break;
        }
    }
    Some(out)
}

fn strip_webp(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 20 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[0..12]);
    let mut i = 12;
    let mut vp8x_at: Option<usize> = None;
    while i + 8 <= bytes.len() {
        let kind = &bytes[i..i + 4];
        let len =
            u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        let total = 8 + len + (len % 2);
        if i + total > bytes.len() {
            return None;
        }
        if kind == b"VP8X" {
            vp8x_at = Some(out.len());
        }
        if kind != b"EXIF" && kind != b"XMP " {
            out.extend_from_slice(&bytes[i..i + total]);
        }
        i += total;
    }
    if let Some(at) = vp8x_at {
        // VP8X feature byte: clear the EXIF (0x08) and XMP (0x04) bits.
        if at + 8 + 4 <= out.len() {
            out[at + 8] &= !0x0C;
        }
    }
    let size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Some(out)
}

// ---------------------------------------------------------------------------
// Base64 decode (the chunk argument; standard alphabet, padded)

pub fn decode_base64(text: &str) -> Result<Vec<u8>, ()> {
    let clean = zeroize::Zeroizing::new(
        text.bytes()
            .filter(|b| !b.is_ascii_whitespace())
            .collect::<Vec<_>>(),
    );
    if clean.is_empty() || clean.len() % 4 != 0 {
        return Err(());
    }
    let value = |c: u8| -> Result<u32, ()> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(()),
        }
    };
    let mut out = zeroize::Zeroizing::new(Vec::with_capacity(clean.len() / 4 * 3));
    for group in clean.chunks(4) {
        let pad = group.iter().filter(|&&c| c == b'=').count();
        if pad > 2 {
            return Err(());
        }
        let mut n: u32 = 0;
        for (i, &c) in group.iter().enumerate() {
            n |= if c == b'=' { 0 } else { value(c)? } << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(std::mem::take(&mut *out))
}

#[cfg(test)]
mod tests {
    use super::finish_upload;
    use crate::error::AppError;
    use crate::state::AppState;
    use soirvu_core::Vault;
    use soirvu_core::crypto::KdfParams;
    use soirvu_core::entries::EntryDraft;

    fn state_with_entry() -> (tempfile::TempDir, AppState, String) {
        let dir = tempfile::TempDir::new().unwrap();
        let vault_dir = dir.path().join("vault");
        let (vault, _recovery) = Vault::create(
            &vault_dir,
            "pass",
            KdfParams {
                m_kib: 64 * 1024,
                t: 3,
                p: 1,
            },
        )
        .unwrap();
        let vault_id = vault.header().vault_id.clone();
        let state = AppState::load(dir.path().join("config"));
        state
            .register(vault_id.clone(), "Test".into(), vault_dir)
            .unwrap();
        state.adopt_created(vault, &vault_id);
        let entry = state
            .with_vault(|vault| {
                vault
                    .create_entry(
                        None,
                        &EntryDraft {
                            entry_date: "2026-10-30".into(),
                            title: Some("Note".into()),
                            body_md: "body".into(),
                            mood: None,
                            tz: None,
                            tags: vec![],
                        },
                    )
                    .map_err(AppError::from)
            })
            .unwrap();
        (dir, state, entry.id)
    }

    /// P1: the rev `attachment_finish` reports must be the entry's
    /// post-commit rev. The editor saves against the rev it opened;
    /// without the reported rev every autosave after an attachment
    /// fails sticky with `revision_conflict`.
    #[test]
    fn finish_reports_the_post_commit_rev() {
        let (_dir, state, entry_id) = state_with_entry();
        let stale = state
            .with_vault(|vault| vault.get_entry(&entry_id).map_err(AppError::from))
            .unwrap()
            .rev;
        let added = finish_upload(
            &state,
            &state.session().unwrap(),
            entry_id.clone(),
            "clip.gif".to_string(),
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec(),
            stale,
        )
        .unwrap();
        assert_eq!(added.rev, stale + 1);
        let changed = EntryDraft {
            entry_date: "2026-10-30".into(),
            title: Some("Note".into()),
            body_md: "edited after attach".into(),
            mood: None,
            tz: None,
            tags: vec![],
        };
        let stale_err = state
            .with_vault(|vault| {
                vault
                    .update_entry(&entry_id, stale, &changed)
                    .map_err(AppError::from)
            })
            .unwrap_err();
        assert_eq!(stale_err.to_ipc(), "revision_conflict");
        state
            .with_vault(|vault| {
                vault
                    .update_entry(&entry_id, added.rev, &changed)
                    .map_err(AppError::from)
            })
            .unwrap();
    }

    /// Follow-up: adopting the upload's rev must not clobber content
    /// that landed while the file uploaded. The finish is conditional
    /// on the editor's base rev — a stale base is refused with the
    /// same conflict a save would hit, and nothing is committed.
    #[test]
    fn finish_refuses_a_stale_base_rev() {
        let (_dir, state, entry_id) = state_with_entry();
        let opened = state
            .with_vault(|vault| vault.get_entry(&entry_id).map_err(AppError::from))
            .unwrap()
            .rev;
        // Sync applies remote content while the file uploads.
        let remote = EntryDraft {
            entry_date: "2026-10-30".into(),
            title: Some("Remote".into()),
            body_md: "remote wins".into(),
            mood: None,
            tz: None,
            tags: vec![],
        };
        state
            .with_vault(|vault| {
                vault
                    .update_entry(&entry_id, opened, &remote)
                    .map_err(AppError::from)
            })
            .unwrap();
        let refused = finish_upload(
            &state,
            &state.session().unwrap(),
            entry_id.clone(),
            "clip.gif".to_string(),
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec(),
            opened,
        )
        .unwrap_err();
        assert_eq!(refused.to_ipc(), "revision_conflict");
        // Nothing committed, remote content intact.
        let kept = state
            .with_vault(|vault| vault.get_entry(&entry_id).map_err(AppError::from))
            .unwrap();
        assert_eq!(kept.body_md, "remote wins");
        let atts = state
            .with_vault(|vault| vault.list_attachments(&entry_id).map_err(AppError::from))
            .unwrap();
        assert!(atts.is_empty());
        // Retrying against the fresh rev commits.
        let added = finish_upload(
            &state,
            &state.session().unwrap(),
            entry_id.clone(),
            "clip.gif".to_string(),
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec(),
            kept.rev,
        )
        .unwrap();
        assert_eq!(added.rev, kept.rev + 1);
    }

    #[test]
    fn preview_and_open_use_attachment_bytes_instead_of_metadata() {
        let (dir, state, entry) = state_with_entry();
        let png = b"\x89PNG\r\n\x1a\n";
        let (known, unknown, pdf) = state
            .with_vault(|vault| {
                Ok((
                    vault.add_attachment(&entry, "note.command", "text/html", png)?,
                    vault.add_attachment(&entry, "other.png", "image/png", b"plain text")?,
                    vault.add_attachment(&entry, "note.txt", "text/plain", b"%PDF-1.7")?,
                ))
            })
            .unwrap();

        let peer_dir = dir.path().join("peer");
        let sync_dir = dir.path().join("sync");
        state
            .with_vault(|vault| {
                vault.checkpoint()?;
                soirvu_core::folder::copy_vault_files(vault.dir(), &peer_dir)?;
                Ok(())
            })
            .unwrap();
        let mut peer = Vault::open_with_passphrase(&peer_dir, "pass").unwrap();
        peer.restamp_device().unwrap();
        let synced = peer
            .add_attachment(&entry, "peer.command", "text/html", png)
            .unwrap();
        peer.sync_emit(&sync_dir).unwrap();
        state
            .with_vault(|vault| {
                let report = vault.sync_apply(&sync_dir)?;
                assert_eq!(report.attachments_applied, 1);
                Ok(())
            })
            .unwrap();

        let response = super::serve_asset(&state, &state.session().unwrap(), &synced.id, Some("bytes=0-999999"));
        assert_eq!(response.status, 206);
        assert_eq!(response.body, png);
        for (name, value) in [
            ("Content-Type", "image/png"),
            ("Content-Range", "bytes 0-7/8"),
            ("Cache-Control", "no-store"),
            ("Content-Security-Policy", "default-src 'none'; sandbox"),
        ] {
            assert!(
                response
                    .headers
                    .iter()
                    .any(|(n, v)| n == name && v == value)
            );
        }
        assert_eq!(
            super::serve_asset(&state, &state.session().unwrap(), &known.id, Some("bytes=999-")).status,
            416
        );
        assert_eq!(super::serve_asset(&state, &state.session().unwrap(), &pdf.id, None).status, 415);

        let response = super::serve_asset(&state, &state.session().unwrap(), &known.id, Some("bytes=1-3"));
        assert_eq!(response.status, 206);
        assert_eq!(response.body, png[1..4]);
        assert!(
            response
                .headers
                .iter()
                .any(|(name, value)| { name == "Content-Type" && value == "image/png" })
        );
        assert_eq!(super::serve_asset(&state, &state.session().unwrap(), &unknown.id, None).status, 415);

        state
            .with_vault(|vault| {
                assert!(matches!(
                    super::materialize_open_copy_locked(vault, &state, &unknown.id),
                    Err(AppError::Code(code)) if code == "unsupported_type"
                ));
                assert!(!state.opened_dir().exists());
                for id in [&known.id, &synced.id] {
                    let path = super::materialize_open_copy_locked(vault, &state, id)?;
                    assert_eq!(path.extension().unwrap(), "png");
                    assert!(
                        path.file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .starts_with("attachment-")
                    );
                    assert_eq!(std::fs::read(path).unwrap(), png);
                }
                let path = super::materialize_open_copy_locked(vault, &state, &pdf.id)?;
                assert_eq!(path.extension().unwrap(), "pdf");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn streamed_attachment_rejects_extra_bytes_instead_of_truncating() {
        let mut exact = super::BoundedAttachmentReader { source: &b"abc"[..], remaining: 3 };
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut exact, &mut bytes).unwrap();
        assert_eq!(bytes, b"abc");

        let mut oversized = super::BoundedAttachmentReader { source: &b"abcd"[..], remaining: 3 };
        let error = std::io::Read::read_to_end(&mut oversized, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
    }

    #[test]
    fn large_assets_require_ranges_and_cap_response_bytes() {
        let (_dir, state, entry) = state_with_entry();
        let id = state.with_vault(|vault| {
            let mut video = b"\0\0\0\x14ftypisom".to_vec();
            video.resize(super::MAX_ASSET_RESPONSE_BYTES as usize + 1, 0);
            Ok(vault.add_attachment(&entry, "clip.mp4", "video/mp4", &video)?.id)
        }).unwrap();

        for range in [None, Some("invalid")] {
            assert_eq!(super::serve_asset(&state, &state.session().unwrap(), &id, range).status, 413);
        }
        let response = super::serve_asset(&state, &state.session().unwrap(), &id, Some("bytes=0-"));
        assert_eq!(response.status, 206);
        assert_eq!(response.body.len() as u64, super::MAX_ASSET_RESPONSE_BYTES);
    }

    #[test]
    fn asset_respects_the_type_cap_for_full_and_range_requests() {
        let (_dir, state, entry) = state_with_entry();
        let id = state
            .with_vault(|vault| {
                let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
                png.resize(1024 * 1024 + 1, 0);
                Ok(vault
                    .add_attachment(&entry, "note.png", "image/png", &png)?
                    .id)
            })
            .unwrap();
        let mut settings = state.settings();
        settings.caps.image_mb = 1;
        state.set_settings(settings).unwrap();
        for range in [None, Some("bytes=0-")] {
            assert_eq!(super::serve_asset(&state, &state.session().unwrap(), &id, range).status, 413);
        }
        assert!(super::cap_for("video/3gpp", &state.settings().caps).is_some());
        assert!(super::cap_for("video/3gpp2", &state.settings().caps).is_some());
    }
}
