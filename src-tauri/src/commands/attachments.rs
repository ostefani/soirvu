//! Attachment commands and the soirvu-asset:// protocol (§6.1, §6.4).
//!
//! Uploads arrive in slices through attachment_chunk (base64 — the
//! isolation pattern carries JSON, not binary bodies), are sniffed
//! and capped in Rust at attachment_finish, have EXIF stripped when
//! the setting says so, and are committed by the core. The UI only
//! ever holds ids.

use std::io::Read;
use std::ops::Range;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{ipc, AppError};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct AttachmentAdded {
    pub id: String,
    pub mime: String,
    pub size: i64,
}

#[derive(Debug, Serialize)]
pub struct AttachmentInfo {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: i64,
}

/// The §6.4 allowlist, sniffed from magic bytes — never from the
/// file name or a UI-supplied type.
pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.len() >= magic.len() && &bytes[..magic.len()] == magic;
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if starts(b"\x89PNG\r\n\x1A\n") {
        return Some("image/png");
    }
    if starts(b"GIF8") {
        return Some("image/gif");
    }
    if starts(b"%PDF-") {
        return Some("application/pdf");
    }
    if starts(b"ID3") || (bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0) {
        return Some("audio/mpeg");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" {
        match &bytes[8..12] {
            b"WEBP" => return Some("image/webp"),
            b"WAVE" => return Some("audio/wav"),
            _ => {}
        }
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        return match brand {
            b"M4A " | b"M4B " => Some("audio/x-m4a"),
            b"heic" | b"heix" | b"hevc" | b"mif1" | b"msf1" => Some("image/heic"),
            b"qt  " => Some("video/quicktime"),
            _ => Some("video/mp4"),
        };
    }
    None
}

/// The size cap (bytes) for a sniffed type, from the settings (§6.4:
/// caps are settings). None when the type is outside the allowlist.
fn cap_for(mime: &str, caps: &crate::state::Caps) -> Option<u64> {
    let mb = |v: u64| v * 1024 * 1024;
    match mime {
        m if m.starts_with("image/") => Some(mb(caps.image_mb)),
        "application/pdf" => Some(mb(caps.pdf_mb)),
        m if m.starts_with("audio/") => Some(mb(caps.audio_mb)),
        m if m.starts_with("video/") => Some(mb(caps.video_mb)),
        _ => None,
    }
}

/// Hard ceiling for one in-flight upload, whatever the settings say:
/// chunks accumulate in memory, so a runaway client must not be able
/// to grow this without bound. Matches the largest §6.4 cap.
pub const MAX_UPLOAD_HARD_CAP: usize = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Chunked upload (§6.1)

#[tauri::command]
pub fn attachment_begin(
    state: State<'_, AppState>,
    entry_id: String,
    name: String,
    mime_hint: Option<String>,
) -> Result<String, String> {
    ipc(state.begin_upload(entry_id, name, mime_hint))
}

#[tauri::command]
pub fn attachment_chunk(
    state: State<'_, AppState>,
    upload_id: String,
    data_base64: String,
) -> Result<(), String> {
    let bytes = match decode_base64(&data_base64) {
        Ok(b) => b,
        Err(()) => return Err("invalid_input".into()),
    };
    ipc(state.push_chunk(&upload_id, &bytes))
}

#[tauri::command(async)]
pub fn attachment_finish(
    state: State<'_, AppState>,
    upload_id: String,
) -> Result<AttachmentAdded, String> {
    let upload = ipc(state.take_upload(&upload_id))?;
    ipc(finish_upload(&state, upload.entry_id, upload.name, upload.bytes))
}

#[tauri::command]
pub fn attachment_abort(state: State<'_, AppState>, upload_id: String) {
    state.drop_upload(&upload_id);
}

/// Sniff, cap, strip, commit — shared by the chunked upload and the
/// file import.
fn finish_upload(
    state: &AppState,
    entry_id: String,
    name: String,
    mut bytes: Vec<u8>,
) -> Result<AttachmentAdded, AppError> {
    let settings = state.settings();
    let mime = sniff_mime(&bytes).ok_or_else(|| AppError::Code("unsupported_type".into()))?;
    let cap = cap_for(mime, &settings.caps).ok_or_else(|| AppError::Code("unsupported_type".into()))?;
    if bytes.len() as u64 > cap {
        return Err(AppError::Code("too_large".into()));
    }
    if settings.strip_exif {
        bytes = strip_metadata(&bytes, mime);
    }
    let added = state.with_vault(|vault| {
        let a = vault.add_attachment(&entry_id, &name, mime, &bytes)?;
        Ok(a)
    })?;
    Ok(AttachmentAdded { id: added.id, mime: added.mime, size: added.size })
}

// ---------------------------------------------------------------------------
// Info, import, save a copy

#[tauri::command(async)]
pub fn attachment_info(
    state: State<'_, AppState>,
    id: String,
) -> Result<AttachmentInfo, String> {
    ipc(
        state.with_vault(|vault| {
            let a = vault.get_attachment(&id)?;
            Ok(AttachmentInfo { id: a.id, name: a.name, mime: a.mime, size: a.size })
        }),
    )
}

/// Optional §6.1 path for very large files: Rust opens the picker
/// and streams the file into the store; the bytes never pass through
/// the webview.
#[tauri::command]
pub async fn attachment_import(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
) -> Result<Option<AttachmentAdded>, String> {
    let Some(path) = pick_file(&app).await else {
        return Ok(None);
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    ipc(import_path(&state, &entry_id, &path, &name).map(Some))
}

pub fn import_path(
    state: &AppState,
    entry_id: &str,
    path: &std::path::Path,
    name: &str,
) -> Result<AttachmentAdded, AppError> {
    let settings = state.settings();
    let mut file = std::fs::File::open(path).map_err(|_| AppError::Code("io".into()))?;
    let mut head = [0u8; 8192];
    let head_len = read_up_to(&mut file, &mut head)?;
    let mime = sniff_mime(&head[..head_len]).ok_or_else(|| AppError::Code("unsupported_type".into()))?;
    let cap = cap_for(mime, &settings.caps).ok_or_else(|| AppError::Code("unsupported_type".into()))?;
    let total = file.metadata().map_err(|_| AppError::Code("io".into()))?.len();
    if total > cap {
        return Err(AppError::Code("too_large".into()));
    }
    let strip = settings.strip_exif && matches!(mime, "image/jpeg" | "image/png" | "image/webp");
    if strip {
        // Strippable kinds are images: their cap (25 MB default)
        // keeps the in-memory pass reasonable.
        let mut bytes = Vec::with_capacity(total as usize);
        bytes.extend_from_slice(&head[..head_len]);
        file.read_to_end(&mut bytes).map_err(|_| AppError::Code("io".into()))?;
        let bytes = strip_metadata(&bytes, mime);
        let added = state.with_vault(|vault| {
            let a = vault.add_attachment(entry_id, name, mime, &bytes)?;
            Ok(a)
        })?;
        return Ok(AttachmentAdded { id: added.id, mime: added.mime, size: added.size });
    }
    let mut reader = std::io::Cursor::new(head[..head_len].to_vec()).chain(file);
    let added = state.with_vault(|vault| {
        let a = vault.add_attachment_from_reader(entry_id, name, mime, &mut reader)?;
        Ok(a)
    })?;
    Ok(AttachmentAdded { id: added.id, mime: added.mime, size: added.size })
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

/// Writes the decrypted attachment to `path`, one range read at
/// a time (the whole file is never held in memory).
pub fn write_copy(state: &AppState, id: &str, path: &std::path::Path) -> Result<(), AppError> {
    use std::io::Write;
    let size = state.with_vault(|vault| {
        let a = vault.get_attachment(id)?;
        Ok(a.size)
    })?;
    let mut out = std::fs::File::create(path).map_err(|_| AppError::Code("io".into()))?;
    let mut offset: u64 = 0;
    let total = size.max(0) as u64;
    while offset < total {
        let end = (offset + 1024 * 1024).min(total);
        let bytes = state.with_vault(|vault| {
            let b = vault.read_attachment_range(id, Range { start: offset, end })?;
            Ok(b)
        })?;
        if bytes.is_empty() {
            break;
        }
        out.write_all(&bytes).map_err(|_| AppError::Code("io".into()))?;
        offset = end;
    }
    Ok(())
}

/// Opens an attachment in the OS default app: the decrypted
/// bytes are written to the app-managed opened folder (wiped on
/// lock) under the file's own name, and the OS takes it from
/// there. The webview never sees the path.
#[tauri::command(async)]
pub fn attachment_open(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    let path = ipc(materialize_open_copy(&state, &id))?;
    Ok(crate::platform::open_external(&path.to_string_lossy()).is_ok())
}

/// Writes the decrypted attachment into opened/<id>/<name> and
/// returns the path (Rust-side only).
pub fn materialize_open_copy(
    state: &AppState,
    id: &str,
) -> Result<std::path::PathBuf, AppError> {
    let info = state.with_vault(|vault| vault.get_attachment(id).map_err(AppError::from))?;
    let safe_name = std::path::Path::new(&info.name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "attachment".to_string());
    let dir = state.opened_dir().join(id);
    std::fs::create_dir_all(&dir).map_err(|_| AppError::Code("io".into()))?;
    let path = dir.join(safe_name);
    write_copy(state, id, &path)?;
    Ok(path)
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
    ];
    for (k, v) in extra {
        headers.push((k.to_string(), v.clone()));
    }
    AssetResponse { status, headers, body }
}

/// Serves one attachment by id. Locked vault → 403; unknown or
/// tombstoned id → 404; a satisfiable Range → 206 with exactly those
/// bytes (§6.4: only the needed chunks are decrypted).
pub fn serve_asset(state: &AppState, id: &str, range_header: Option<&str>) -> AssetResponse {
    let text = |status: u16, msg: &str| asset(status, "text/plain", msg.as_bytes().to_vec(), &[]);
    let info = match state.with_vault(|vault| vault.get_attachment(id).map_err(AppError::from)) {
        Ok(info) => info,
        Err(AppError::Code(code)) if code == "locked" => return text(403, "locked"),
        Err(_) => return text(404, "not found"),
    };
    let total = info.size.max(0) as u64;
    if let Some(header) = range_header {
        if let Some((start, _)) = parse_range(header, total) {
            if start >= total && total > 0 {
                return asset(
                    416,
                    "text/plain",
                    Vec::new(),
                    &[("Content-Range", format!("bytes */{total}"))],
                );
            }
        }
    }
    let range = range_header.and_then(|h| parse_range(h, total));
    match range {
        Some((start, end)) => {
            let bytes = state.with_vault(|vault| {
                let b = vault.read_attachment_range(id, Range { start, end: end + 1 })?;
                Ok(b)
            });
            match bytes {
                Ok(bytes) => asset(
                    206,
                    &info.mime,
                    bytes,
                    &[("Content-Range", format!("bytes {start}-{end}/{total}"))],
                ),
                _ => text(404, "not found"),
            }
        }
        None => {
            let bytes = state.with_vault(|vault| {
                let b = vault.read_attachment_range(id, Range { start: 0, end: total })?;
                Ok(b)
            });
            match bytes {
                Ok(bytes) => asset(200, &info.mime, bytes, &[]),
                _ => text(404, "not found"),
            }
        }
    }
}

/// Parses a single `bytes=start-end` range against the body size
/// (suffix and open-ended forms included), clamped to the last
/// byte. Anything malformed → None → the whole body is served.
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
        let len = u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
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
    let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
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
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
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
    Ok(out)
}
