// SPDX-License-Identifier: MIT
use objc2::{
    ClassType, MainThreadMarker, ffi, msg_send,
    rc::Retained,
    runtime::{AnyObject, Imp, NSObjectProtocol, Sel},
    sel,
};
use objc2_app_kit::NSWritingToolsBehavior;
use objc2_foundation::NSProcessInfo;
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};
use std::io;
use tauri::webview::PlatformWebview;

pub fn configuration() -> tauri::Result<Retained<WKWebViewConfiguration>> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| io::Error::other("WKWebView must be configured on the main thread"))?;
    // SAFETY: The configuration is created and used on AppKit's main thread.
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    if supports_writing_tools() {
        if !config.respondsToSelector(sel!(setWritingToolsBehavior:)) {
            return Err(io::Error::other("WKWebView Writing Tools setter is unavailable").into());
        }
        // SAFETY: The selector was checked and takes NSWritingToolsBehavior.
        unsafe {
            let () = msg_send![&config, setWritingToolsBehavior: NSWritingToolsBehavior::None];
        }
    }
    Ok(config)
}

fn supports_writing_tools() -> bool {
    NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion
        >= 15
}

pub(super) fn install(webview: &PlatformWebview) -> tauri::Result<()> {
    // SAFETY: Tauri lends the live WKWebView on AppKit's main thread.
    let native = unsafe { &*webview.inner().cast::<WKWebView>() };
    // SAFETY: This getter reads the preview policy of the same live view.
    if unsafe { native.allowsLinkPreview() } {
        return Err(io::Error::other("WKWebView link previews are enabled").into());
    }
    if supports_writing_tools() {
        // SAFETY: Read the copied configuration of the live view on the main thread.
        let config = unsafe { native.configuration() };
        if !config.respondsToSelector(sel!(writingToolsBehavior)) {
            return Err(io::Error::other("WKWebView Writing Tools getter is unavailable").into());
        }
        let behavior: NSWritingToolsBehavior = unsafe { msg_send![&config, writingToolsBehavior] };
        if behavior != NSWritingToolsBehavior::None {
            return Err(
                io::Error::other("WKWebView Writing Tools opt-out did not take effect").into(),
            );
        }
    }
    let view: &AnyObject = native;
    let live_class = view.class();
    // Patch only Wry's application-owned subclass. Changing the live view's
    // class after AppKit has attached it invalidates its dynamic properties.
    let mut class = live_class;
    while class
        .name()
        .to_bytes()
        .windows(b"NSKVONotifying_".len())
        .any(|part| part == b"NSKVONotifying_")
    {
        class = class
            .superclass()
            .ok_or_else(|| io::Error::other("WKWebView KVO class has no superclass"))?;
    }
    if !is_wry_webview_name(class.name().to_bytes())
        || class.superclass() != Some(WKWebView::class())
    {
        return Err(io::Error::other(format!(
            "unexpected WKWebView class: {}",
            class.name().to_string_lossy()
        ))
        .into());
    }
    // SAFETY: Each callback has the ABI of its NSResponder selector. Their
    // type encodings come from the existing methods; installation is on the
    // main thread and does not affect WKWebView or other AppKit classes.
    unsafe {
        type Lookup = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);
        type Service =
            extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject, *mut AnyObject) -> *mut AnyObject;
        let lookup = std::mem::transmute::<Lookup, Imp>(ignore_lookup);
        let service = std::mem::transmute::<Service, Imp>(refuse_service);
        for (selector, implementation, encoding) in [
            (sel!(quickLookWithEvent:), lookup, c"v@:@"),
            (sel!(quickLookPreviewItems:), lookup, c"v@:@"),
            (
                sel!(validRequestorForSendType:returnType:),
                service,
                c"@@:@@",
            ),
        ] {
            let method = class.instance_method(selector);
            if !method
                .is_some_and(|method| std::ptr::fn_addr_eq(method.implementation(), implementation))
            {
                ffi::class_replaceMethod(
                    std::ptr::from_ref(class).cast_mut(),
                    selector,
                    implementation,
                    method.map_or(encoding.as_ptr(), |method| {
                        ffi::method_getTypeEncoding(method)
                    }),
                );
            }
            if !live_class
                .instance_method(selector)
                .is_some_and(|method| std::ptr::fn_addr_eq(method.implementation(), implementation))
            {
                return Err(io::Error::other(format!(
                    "WKWebView responder guard is missing: {selector:?}"
                ))
                .into());
            }
        }
    }
    Ok(())
}

fn is_wry_webview_name(name: &[u8]) -> bool {
    // objc2's default name is module_path::Type followed by the crate version.
    let leaf = name.rsplit(|&byte| byte == b':').next().unwrap_or_default();
    leaf.strip_prefix(b"WryWebView").is_some_and(|version| {
        version.is_empty() || version.first().is_some_and(u8::is_ascii_digit)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn wry_class_identity_does_not_depend_on_its_module() {
        for name in [
            b"WryWebView".as_slice(),
            b"wry::relocated::WryWebView0.57.0",
        ] {
            assert!(super::is_wry_webview_name(name));
        }
        for name in [b"WKWebView".as_slice(), b"wry::WryWebViewParent0.57.0"] {
            assert!(!super::is_wry_webview_name(name));
        }
    }
}

extern "C-unwind" fn ignore_lookup(_: &AnyObject, _: Sel, _: *mut AnyObject) {}

extern "C-unwind" fn refuse_service(
    _: &AnyObject,
    _: Sel,
    _: *mut AnyObject,
    _: *mut AnyObject,
) -> *mut AnyObject {
    std::ptr::null_mut()
}

pub fn app_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{
        AboutMetadata, HELP_SUBMENU_ID, Menu, PredefinedMenuItem as Item, Submenu,
        WINDOW_SUBMENU_ID,
    };
    let info = app.package_info();
    let about = AboutMetadata {
        name: Some(info.name.clone()),
        version: Some(info.version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        authors: app
            .config()
            .bundle
            .publisher
            .clone()
            .map(|publisher| vec![publisher]),
        ..Default::default()
    };
    // Build without a Services item instead of assuming AppKit will keep
    // its internal Services menu reference nil.
    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                &info.name,
                true,
                &[
                    &Item::about(app, None, Some(about))?,
                    &Item::separator(app)?,
                    &Item::hide(app, None)?,
                    &Item::hide_others(app, None)?,
                    &Item::separator(app)?,
                    &Item::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(app, "File", true, &[&Item::close_window(app, None)?])?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &Item::undo(app, None)?,
                    &Item::redo(app, None)?,
                    &Item::separator(app)?,
                    &Item::cut(app, None)?,
                    &Item::copy(app, None)?,
                    &Item::paste(app, None)?,
                    &Item::select_all(app, None)?,
                ],
            )?,
            &Submenu::with_items(app, "View", true, &[&Item::fullscreen(app, None)?])?,
            &Submenu::with_id_and_items(
                app,
                WINDOW_SUBMENU_ID,
                "Window",
                true,
                &[
                    &Item::minimize(app, None)?,
                    &Item::maximize(app, None)?,
                    &Item::separator(app)?,
                    &Item::close_window(app, None)?,
                ],
            )?,
            &Submenu::with_id_and_items(app, HELP_SUBMENU_ID, "Help", true, &[])?,
        ],
    )
}
