// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Linux lock signals (DESIGN §12): logind and the GNOME
//! screensaver over D-Bus, without a D-Bus crate — `gdbus
//! monitor` subprocesses (gdbus ships with GLib, present
//! wherever WebKitGTK is) whose printed lines are matched
//! by `signal_locks`.
//!
//! Two monitors: the system bus for logind (PrepareForSleep
//! and the session Lock signal), the session bus for
//! GNOME's screensaver (ActiveChanged). A monitor that
//! cannot start, or exits, degrades that one signal source
//! silently: the idle timer stays as the fallback, and a
//! missing gdbus must never break startup or locking.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use crate::platform::{LockEvents, LockReason};

/// What one gdbus monitor line means for the diary, if
/// anything. gdbus prints signals as
/// `/path: interface.member (args)`; the forms matched:
///   org.freedesktop.login1.Manager.PrepareForSleep (true,)
///   org.freedesktop.login1.Session.Lock ()
///   org.gnome.ScreenSaver.ActiveChanged (true,)
/// PrepareForSleep(false) is the wake-up and Session.Unlock
/// is the user returning — neither locks.
pub(crate) fn signal_locks(line: &str) -> Option<LockReason> {
    if line.contains("org.freedesktop.login1.Manager.PrepareForSleep") {
        return line.contains("(true").then_some(LockReason::Sleep);
    }
    if line.contains("org.freedesktop.login1.Session.Lock") {
        return Some(LockReason::ScreenLocked);
    }
    if line.contains("org.gnome.ScreenSaver.ActiveChanged") {
        return line.contains("(true").then_some(LockReason::Screensaver);
    }
    None
}

#[derive(Default)]
pub struct LinuxLockEvents {
    children: Vec<Child>,
}

impl LinuxLockEvents {
    /// Spawn one monitor and pump its lines into `on_lock`
    /// from a reader thread. Returns false when gdbus is
    /// missing or refuses (no bus, no such destination).
    fn watch(
        &mut self,
        bus_args: &[&str],
        dest: &str,
        on_lock: &Arc<Box<dyn Fn(LockReason) + Send + Sync>>,
    ) -> bool {
        let mut cmd = Command::new("gdbus");
        cmd.arg("monitor")
            .args(bus_args)
            .arg("--dest")
            .arg(dest)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(_) => return false,
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            return false;
        };
        self.children.push(child);
        let on_lock = Arc::clone(on_lock);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(reason) = signal_locks(&line) {
                    on_lock(reason);
                }
            }
        });
        true
    }
}

impl LockEvents for LinuxLockEvents {
    fn start(
        &mut self,
        on_lock: Box<dyn Fn(LockReason) + Send + Sync>,
    ) -> Result<(), String> {
        let on_lock = Arc::new(on_lock);
        if !self.watch(&["--system"], "org.freedesktop.login1", &on_lock) {
            eprintln!("linux lock events: no logind monitor (idle lock only)");
        }
        if !self.watch(&["--session"], "org.gnome.ScreenSaver", &on_lock) {
            eprintln!("linux lock events: no screensaver monitor (idle lock only)");
        }
        Ok(())
    }
}

impl Drop for LinuxLockEvents {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_true_locks_false_does_not() {
        assert_eq!(
            signal_locks(
                "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (true,)"
            ),
            Some(LockReason::Sleep)
        );
        assert_eq!(
            signal_locks(
                "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (false,)"
            ),
            None
        );
    }

    #[test]
    fn session_lock_locks_unlock_does_not() {
        assert_eq!(
            signal_locks(
                "/org/freedesktop/login1/session/_31: org.freedesktop.login1.Session.Lock ()"
            ),
            Some(LockReason::ScreenLocked)
        );
        assert_eq!(
            signal_locks(
                "/org/freedesktop/login1/session/_31: org.freedesktop.login1.Session.Unlock ()"
            ),
            None
        );
    }

    #[test]
    fn screensaver_true_locks() {
        assert_eq!(
            signal_locks("/org/gnome/ScreenSaver: org.gnome.ScreenSaver.ActiveChanged (true,)"),
            Some(LockReason::Screensaver)
        );
        assert_eq!(
            signal_locks("/org/gnome/ScreenSaver: org.gnome.ScreenSaver.ActiveChanged (false,)"),
            None
        );
    }

    #[test]
    fn unrelated_lines_do_not_lock() {
        assert_eq!(signal_locks(""), None);
        assert_eq!(
            signal_locks("/org/freedesktop/login1: org.freedesktop.login1.Manager.CanReboot ()"),
            None
        );
    }

    #[test]
    fn start_never_fails() {
        // With or without a bus in the test environment,
        // start() degrades and reports success.
        let mut events = LinuxLockEvents::default();
        assert!(events.start(Box::new(|_| {})).is_ok());
    }
}
