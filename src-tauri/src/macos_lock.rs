//! macOS lock signals (§9): NSWorkspace notifications for sleep
//! and display sleep, distributed notifications for screen lock
//! and screensaver start — behind the platform LockEvents
//! trait, so nothing outside this file names an Objective-C
//! type. Observers are registered once and live for the process
//! lifetime: the app never stops listening for a lock signal.

use std::ptr::NonNull;
use std::sync::Arc;

use block2::RcBlock;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceScreensDidSleepNotification, NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSNotificationName,
    NSOperationQueue,
};

use crate::platform::{LockEvents, LockReason};

type Callback = Arc<dyn Fn(LockReason) + Send + Sync>;

#[derive(Default)]
pub struct MacLockEvents;

/// Registers one observer and returns its token. Dropping the
/// token would unregister the observer; start() keeps them all
/// and forgets them on purpose (below).
fn observe(
    center: &NSNotificationCenter,
    name: &NSNotificationName,
    callback: &Callback,
    reason: LockReason,
) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    let callback = callback.clone();
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
        callback(reason);
    });
    unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(name),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &*block,
        )
    }
}

impl LockEvents for MacLockEvents {
    fn start(&mut self, on_lock: Box<dyn Fn(LockReason) + Send + Sync>) -> Result<(), String> {
        let callback: Callback = Arc::from(on_lock);

        let mut tokens = Vec::new();

        // Sleep and display sleep (a lid close ends in sleep).
        let workspace_center = NSWorkspace::sharedWorkspace().notificationCenter();
        unsafe {
            tokens.push(observe(
                &workspace_center,
                NSWorkspaceWillSleepNotification,
                &callback,
                LockReason::Sleep,
            ));
            tokens.push(observe(
                &workspace_center,
                NSWorkspaceScreensDidSleepNotification,
                &callback,
                LockReason::Sleep,
            ));
        }

        // Screen lock and screensaver are distributed
        // notifications posted by the login session.
        let distributed = NSDistributedNotificationCenter::defaultCenter();
        tokens.push(observe(
            &distributed,
            &NSNotificationName::from_str("com.apple.screenIsLocked"),
            &callback,
            LockReason::ScreenLocked,
        ));
        tokens.push(observe(
            &distributed,
            &NSNotificationName::from_str("com.apple.screensaver.didstart"),
            &callback,
            LockReason::Screensaver,
        ));

        // The tokens are the registrations: dropping them would
        // unregister the observers, and there is no moment where
        // the app stops listening for a lock signal — so they
        // are forgotten on purpose and live for the process.
        // (Keeping them in the struct instead would make the
        // backend neither Send nor Sync for no benefit.)
        std::mem::forget(tokens);
        Ok(())
    }
}
