// SPDX-License-Identifier: MIT
use objc2::{
    MainThreadMarker, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, ClassBuilder, NSObjectProtocol, Sel},
    sel,
};
use objc2_web_kit::WKWebViewConfiguration;
use std::{io, sync::OnceLock};
use tauri::webview::PlatformWebview;

pub fn configuration() -> tauri::Result<Retained<WKWebViewConfiguration>> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| io::Error::other("WKWebView must be configured on the main thread"))?;
    // SAFETY: The configuration is created and used on AppKit's main thread.
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    if config.respondsToSelector(sel!(setWritingToolsBehavior:)) {
        // SAFETY: This public macOS 15+ property takes NSWritingToolsBehavior
        // (NSInteger); NSTextCheckingClient.h defines None as -1.
        unsafe {
            let () = msg_send![&config, setWritingToolsBehavior: -1_isize];
        }
    }
    Ok(config)
}

pub(super) fn install(webview: &PlatformWebview) -> tauri::Result<()> {
    static CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
    // SAFETY: Tauri lends the live WKWebView on AppKit's main thread.
    let view = unsafe { &*webview.inner().cast::<AnyObject>() };
    let class = CLASS
        .get_or_init(|| {
            let mut class = ClassBuilder::new(c"SoirVuPrivateWebView", view.class())?;
            // SAFETY: quickLookWithEvent: is an NSResponder method taking one
            // object argument and returning void. The subclass adds no storage.
            // validRequestorForSendType:returnType: takes two objects and returns an object.
            unsafe {
                class.add_method(
                    sel!(quickLookWithEvent:),
                    ignore_lookup as extern "C-unwind" fn(_, _, _),
                );
                class.add_method(
                    sel!(quickLookPreviewItems:),
                    ignore_lookup as extern "C-unwind" fn(_, _, _),
                );
                class.add_method(
                    sel!(validRequestorForSendType:returnType:),
                    refuse_service as extern "C-unwind" fn(_, _, _, _) -> _,
                );
            }
            Some(class.register())
        })
        .ok_or_else(|| io::Error::other("could not install the WKWebView lookup guard"))?;
    // SAFETY: The new class directly inherits Wry's class with identical layout.
    if class.superclass() != Some(view.class()) {
        return Err(io::Error::other("unexpected WKWebView class").into());
    }
    unsafe {
        AnyObject::set_class(view, class);
    }
    Ok(())
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

pub(super) fn disable_services_menu() -> tauri::Result<()> {
    use objc2_app_kit::NSApplication;
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| io::Error::other("AppKit menus require the main thread"))?;
    NSApplication::sharedApplication(mtm).setServicesMenu(None);
    Ok(())
}
