fn main() {
    // §6.3: by default every registered command is callable by
    // every window. Restrict the IPC surface to exactly the
    // commands of §6.1 that this build implements.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "vault_list",
        "vault_pick_folder",
        "vault_create",
        "vault_unlock",
        "vault_lock",
        "vault_forget",
        "vault_relocate",
        "entry_list",
        "calendar_days",
        "entry_get",
        "entry_save",
        "entry_delete",
        "search",
        "tag_list",
        "tag_rename",
        "tag_merge",
        "open_link",
        "clipboard_note_copy",
        "settings_get",
        "settings_set",
        "settings_pick_backup_dir",
        "settings_clear_backup_dir",
        "attachment_begin",
        "attachment_chunk",
        "attachment_finish",
        "attachment_abort",
        "attachment_info",
        "attachment_import",
        "attachment_open"
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to run tauri-build");
}
