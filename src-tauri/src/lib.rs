//! SoirVu's Tauri shell: a thin bridge between the webview and
//! soirVu-core. All diary logic lives in the core; this crate owns
//! the lock state machine, the command surface (§6.1) and the
//! security configuration (§6.3).

mod backup;
mod commands;
mod error;
mod keychain;
#[cfg(target_os = "linux")]
mod linux_lock;
mod lock_handshake;
#[cfg(target_os = "macos")]
mod macos_lock;
mod platform;
mod state;
#[cfg(target_os = "windows")]
mod windows_lock;

use std::time::Duration;

use tauri::{Emitter, Manager};

use state::AppState;

use lock_handshake::{LOCK_ACK_WAIT, LOCK_ACK_WAIT_OS};

/// The app-data folder under the system config dir, holding
/// app.json (the vault registry and settings) and opened/.
/// Debug builds (tauri dev) get their own folder, so a dev
/// session can never see, register over or wipe the release
/// app's data by accident; release builds — including the
/// signed Touch ID variant — use the plain name.
const CONFIG_DIR_NAME: &str = if cfg!(debug_assertions) {
    "SoirVu Dev"
} else {
    "SoirVu"
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // §9 release hardening first, before anything opens.
    platform::harden_process();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // soirvu-asset://<attachment_id> (§6.4): serves decrypted
        // attachment bytes by id only — never a path — and only
        // while the vault is unlocked. Range requests get a 206
        // with exactly the needed chunks decrypted.
        .register_asynchronous_uri_scheme_protocol("soirvu-asset", |ctx, request, responder| {
            use tauri::Manager;
            let app = ctx.app_handle().clone();
            let uri = request.uri();
            let path_id = uri.path().trim_matches('/');
            let id: String = if !path_id.is_empty() && !path_id.contains('/') {
                path_id.to_string()
            } else {
                uri.host().unwrap_or("").to_string()
            };
            let range = request
                .headers()
                .get(tauri::http::header::RANGE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string());
            tauri::async_runtime::spawn(async move {
                let answer = tauri::async_runtime::spawn_blocking(move || {
                    if !crate::commands::attachments::valid_attachment_id(&id) {
                        crate::commands::attachments::AssetResponse {
                            status: 400,
                            headers: vec![("Content-Length".into(), "0".into())],
                            body: Vec::new(),
                        }
                    } else {
                        let state = app.state::<crate::state::AppState>();
                        crate::commands::attachments::serve_asset(&state, &id, range.as_deref())
                    }
                })
                .await
                .unwrap_or_else(|_| crate::commands::attachments::AssetResponse {
                    status: 500,
                    headers: vec![("Content-Length".into(), "0".into())],
                    body: Vec::new(),
                });
                let mut builder = tauri::http::Response::builder().status(answer.status);
                for (name, value) in &answer.headers {
                    builder = builder.header(name.as_str(), value.as_str());
                }
                responder.respond(
                    builder
                        .body(answer.body)
                        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new())),
                );
            });
        })
        .setup(|app| {
            // App config (§3): ~/Library/Application Support/<name>/
            let config_dir = app.path().config_dir()?.join(CONFIG_DIR_NAME);
            app.manage(AppState::load(config_dir));

            // The main window is built in code, not in
            // tauri.conf.json: the navigation guard attaches here,
            // and this is where the non-persistent data store will
            // be set once spike 5 confirms the option (§6.3).
            let builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("SoirVu")
            .inner_size(1100.0, 760.0)
            .min_inner_size(720.0, 480.0)
            .on_navigation(|url| {
                // Only the app's own pages may load. Anything else
                // is blocked; content links go through open_link.
                // The plain localhost allowance is the dev server
                // only: in a release build the webview must not
                // be able to navigate to any local service.
                let is_app_origin = url.scheme() == "tauri"
                    || (cfg!(target_os = "windows")
                        && matches!(url.scheme(), "http" | "https")
                        && url.host_str() == Some("tauri.localhost"));
                let is_dev_server = cfg!(debug_assertions)
                    && url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
                is_app_origin || is_dev_server
            });
            // The editor handles drops itself (files, block
            // drags) through the page's own events; Tauri's
            // native handler replaces WebView2's and would
            // swallow them on Windows (§12). Gated to Windows:
            // the other engines deliver the page's events
            // already, and their native path stays untouched.
            // platform-ok: a WebView2-specific builder option.
            #[cfg(target_os = "windows")]
            let builder = builder.disable_drag_drop_handler();
            // WebKitGTK's default data manager persists site
            // data under ~/.local/share, and WebView2 keeps a
            // profile folder; the ephemeral flag keeps the
            // diary's webview stateless on both (§6.3, §12).
            // platform-ok: the ephemeral flag is a builder option.
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            let builder = builder.incognito(true);
            let window = builder.build()?;
            let _ = window.set_focus();

            // OS lock signals (§9): system sleep, screen lock and
            // screensaver lock the vault like the idle timer does.
            let mut lock_events = platform::lock_events();
            let event_handle = app.handle().clone();
            let _ = lock_events.start(Box::new(move |reason| {
                let state = event_handle.state::<AppState>();
                if state.status() != state::Status::Unlocked {
                    return;
                }
                // Sleep locks at once, as before the handshake: the
                // keys must be gone before the machine sleeps. (Lid
                // close sleeps, so it arrives as Sleep, not as its
                // own variant.) Screen lock and screensaver wait at
                // most a second for the UI to flush.
                if matches!(reason, platform::LockReason::Sleep) {
                    let _ = commands::vault::lock_forced(&event_handle, None);
                } else {
                    let handle = event_handle.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        let _ = commands::vault::lock_forced(&handle, Some(LOCK_ACK_WAIT_OS));
                    });
                }
            }));
            // The observers live as long as the process: there
            // is no moment where the app stops listening.
            std::mem::forget(lock_events);

            // Idle auto-lock (§6.2): every 5 s, ask the state; on
            // expiry, ask the UI to flush, then lock and tell the UI.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut ticks: u64 = 0;
                let mut was_unlocked = false;
                loop {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    ticks += 1;
                    let check_handle = handle.clone();
                    let (idle, unlocked) = tauri::async_runtime::spawn_blocking(move || {
                        let state = check_handle.state::<AppState>();
                        let idle = state.idle_expired();
                        // §3: a volume that disappears mid-session locks
                        // the vault and drops its keys. No handshake
                        // here: with the volume gone there is nothing
                        // to flush to.
                        if state.lock_if_current_missing() {
                            commands::emit_status(&check_handle, &state);
                            commands::reload_main_window(&check_handle);
                        }
                        (idle, state.status() == state::Status::Unlocked)
                    })
                    .await
                    .unwrap_or((false, false));
                    if idle {
                        let lock_handle = handle.clone();
                        let _ = tauri::async_runtime::spawn_blocking(move || {
                            if commands::vault::lock_forced(&lock_handle, Some(LOCK_ACK_WAIT)) {
                                let _ = lock_handle.emit("vault-locked", "idle");
                            }
                        })
                        .await;
                    }
                    let unlocked = unlocked
                        && handle.state::<AppState>().status() == state::Status::Unlocked;
                    // §13: scheduled backups — once when a vault opens,
                    // then every five minutes while open. Detached so a
                    // large copy cannot stall lock and idle checks.
                    if (unlocked && !was_unlocked) || ticks % 60 == 0 {
                        let backup_handle = handle.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            let state = backup_handle.state::<AppState>();
                            crate::backup::maybe_scheduled(&state);
                        });
                    }
                    was_unlocked = unlocked;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault::vault_list,
            commands::vault::vault_pick_folder,
            commands::vault::vault_create,
            commands::vault::vault_unlock,
            commands::vault::vault_lock,
            commands::vault::vault_lock_ready,
            commands::vault::lock_notice_take,
            commands::vault::vault_activity,
            commands::vault::vault_forget,
            commands::vault::vault_relocate,
            commands::vault::vault_fork_import,
            commands::vault::vault_change_passphrase,
            commands::vault::vault_regenerate_recovery,
            commands::vault::vault_rotate_master_key,
            commands::vault::vault_unlock_touch_id,
            commands::vault::touch_id_status,
            commands::vault::touch_id_enroll,
            commands::vault::touch_id_disable,
            commands::entries::entry_list,
            commands::entries::calendar_days,
            commands::entries::on_this_day,
            commands::entries::entry_get,
            commands::entries::entry_save,
            commands::entries::entry_delete,
            commands::entries::search,
            commands::entries::tag_list,
            commands::entries::tag_rename,
            commands::entries::tag_merge,
            commands::entries::tag_delete,
            commands::misc::open_link,
            commands::misc::clipboard_note_copy,
            commands::misc::backup_now,
            commands::misc::export_markdown,
            commands::misc::export_json,
            commands::misc::import_markdown,
            commands::misc::import_json,
            commands::misc::settings_get,
            commands::misc::settings_set,
            commands::misc::settings_pick_backup_dir,
            commands::misc::settings_clear_backup_dir,
            commands::sync::sync_overview,
            commands::sync::sync_pick_folder,
            commands::sync::sync_clear_folder,
            commands::sync::sync_emit,
            commands::sync::sync_apply,
            commands::sync::sync_prune,
            commands::sync::sync_reemit,
            commands::sync::sync_dismiss_ack_ahead,
            commands::sync::restamp_device,
            commands::attachments::attachment_begin,
            commands::attachments::attachment_chunk,
            commands::attachments::attachment_finish,
            commands::attachments::attachment_abort,
            commands::attachments::attachment_info,
            commands::attachments::attachment_import,
            commands::attachments::attachment_open,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SoirVu");
}
