// SPDX-License-Identifier: MIT
use tauri::WebviewWindow;

// platform-ok: WKWebView-only defaults and force gesture.
#[cfg(target_os = "macos")]
pub const MACOS_POLICY: &str = include_str!("context_menu/macos_policy.js");

// platform-ok: WebKit configuration and the AppKit responder guard are macOS-only.
#[cfg(target_os = "macos")]
#[path = "context_menu/macos.rs"]
mod macos;

// platform-ok: configure Writing Tools before creating WKWebView.
#[cfg(target_os = "macos")]
pub use macos::configuration as macos_configuration;

// platform-ok: catch native lookup independently of DOM events.
#[cfg(target_os = "macos")]
pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
    install_before_show(window, macos::install)?;
    macos::disable_services_menu()
}

// platform-ok: WebKitGTK provides a native menu signal.
#[cfg(target_os = "linux")]
pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
    use webkit2gtk::{
        ContextMenuAction, ContextMenuExt, ContextMenuItemExt, SettingsExt, WebViewExt,
    };

    window.with_webview(|webview| {
        let view = webview.inner();
        if let Some(settings) = view.settings() {
            settings.set_enable_dns_prefetching(false);
            settings.set_enable_hyperlink_auditing(false);
        }
        view.connect_context_menu(|_, menu, _, _| {
            for item in menu.items() {
                if !matches!(
                    item.stock_action(),
                    ContextMenuAction::Cut | ContextMenuAction::Copy | ContextMenuAction::Paste
                ) {
                    menu.remove(&item);
                }
            }
            menu.n_items() == 0
        });
    })
}

// platform-ok: WebView2 registers its menu policy on the COM thread.
#[cfg(target_os = "windows")]
pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
    install_before_show(window, |webview| {
        webview2::install(webview).map_err(|err| {
            std::io::Error::other(format!("WebView2 privacy setup failed: {err}")).into()
        })
    })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn install_before_show(
    window: &WebviewWindow,
    configure: impl FnOnce(&tauri::webview::PlatformWebview) -> tauri::Result<()> + Send + 'static,
) -> tauri::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    window.with_webview(move |webview| {
        let _ = tx.send(configure(&webview));
    })?;
    // setup runs on the main thread, where with_webview calls back inline.
    rx.try_recv().map_err(|err| {
        std::io::Error::other(format!(
            "native privacy setup did not complete on the main thread: {err}"
        ))
    })?
}

// platform-ok: WebView2 COM types are Windows-only.
#[cfg(target_os = "windows")]
mod webview2 {
    use tauri::webview::PlatformWebview;
    use webview2_com::{
        ContextMenuRequestedEventHandler,
        Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND, ICoreWebView2_11,
            ICoreWebView2ContextMenuRequestedEventArgs, ICoreWebView2Settings3,
            ICoreWebView2Settings4,
        },
        take_pwstr,
    };
    use windows_core::Interface;

    pub(super) fn install(webview: &PlatformWebview) -> windows_core::Result<()> {
        // SAFETY: Tauri calls this on the WebView2 apartment thread in setup.
        unsafe {
            let core = webview.controller().CoreWebView2()?;
            let settings = core.Settings()?;
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings3>() {
                settings.SetAreBrowserAcceleratorKeysEnabled(false)?;
            }
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings4>() {
                settings.SetIsGeneralAutofillEnabled(false)?;
                settings.SetIsPasswordAutosaveEnabled(false)?;
            }
            // Enable stock menu presentation only after installing the allowlist.
            settings.SetAreDefaultContextMenusEnabled(false)?;
            // Older WebView2 runtimes can safely run without a context menu.
            let Ok(core) = core.cast::<ICoreWebView2_11>() else {
                return Ok(());
            };
            let handler = ContextMenuRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    // If filtering fails partway through, never show the remainder.
                    args.SetHandled(true)?;
                    if filter_items(&args)? {
                        args.SetHandled(false)?;
                    }
                }
                Ok(())
            }));
            let mut token = 0;
            core.add_ContextMenuRequested(&handler, &mut token)?;
            settings.SetAreDefaultContextMenusEnabled(true)
        }
    }

    fn filter_items(
        args: &ICoreWebView2ContextMenuRequestedEventArgs,
    ) -> windows_core::Result<bool> {
        // SAFETY: Only called from ContextMenuRequested, on the owning thread.
        unsafe {
            let items = args.MenuItems()?;
            let mut count = 0;
            items.Count(&mut count)?;
            let mut has_edit_item = false;
            for index in (0..count).rev() {
                let item = items.GetValueAtIndex(index)?;
                let mut kind = Default::default();
                item.Kind(&mut kind)?;
                if kind == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND {
                    let mut name = Default::default();
                    item.Name(&mut name)?;
                    if matches!(take_pwstr(name).as_str(), "cut" | "copy" | "paste") {
                        has_edit_item = true;
                        continue;
                    }
                }
                items.RemoveValueAtIndex(index)?;
            }
            Ok(has_edit_item)
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn install(_: &WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn show_edit_menu(
    window: WebviewWindow,
    x: u32,
    y: u32,
    copy: bool,
    cut: bool,
    paste: bool,
) -> Result<(), &'static str> {
    // platform-ok: macOS uses an app-built popup routed through the first responder.
    #[cfg(target_os = "macos")]
    {
        use tauri::{LogicalPosition, menu::MenuBuilder};

        if !window.is_focused().unwrap_or(false) || !(copy || cut || paste) {
            return Ok(());
        }
        let mut builder = MenuBuilder::new(&window);
        if cut {
            builder = builder.cut();
        }
        if copy {
            builder = builder.copy();
        }
        if paste {
            builder = builder.paste();
        }
        let menu = builder.build().map_err(|_| "internal")?;
        window
            .popup_menu_at(&menu, LogicalPosition::new(f64::from(x), f64::from(y)))
            .map_err(|_| "internal")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, x, y, copy, cut, paste);
        Ok(())
    }
}
