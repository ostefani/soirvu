//! Windows lock signals (DESIGN §12): session lock and
//! system suspend, via a message-only window on a dedicated
//! thread — raw FFI, no windows crate. The window registers
//! for WTS session notifications (WM_WTSSESSION_CHANGE) and
//! suspend/resume (WM_POWERBROADCAST); both arrive on the
//! thread's own message loop, and the trait callback runs
//! there (the platform contract allows a worker thread).
//!
//! If window creation or a registration fails, the backend
//! degrades silently: the idle timer remains as the
//! fallback, and locking signals must never break startup.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use crate::platform::{LockEvents, LockReason};

type Hwnd = *mut c_void;
type Lresult = isize;
type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> Lresult;

const WM_QUIT: u32 = 0x0012;
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const PBT_APMSUSPEND: usize = 0x0004;
const WTS_SESSION_LOCK: usize = 0x0007;
const NOTIFY_FOR_THIS_SESSION: u32 = 0;
const DEVICE_NOTIFY_WINDOW_HANDLE: u32 = 0;
/// A message-only window: its parent is HWND_MESSAGE.
const HWND_MESSAGE: Hwnd = -3isize as Hwnd;

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: Point,
}

#[repr(C)]
struct WndClassW {
    style: u32,
    lpfn_wnd_proc: Option<WndProc>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: *mut c_void,
    h_icon: *mut c_void,
    h_cursor: *mut c_void,
    hbr_background: *mut c_void,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
}

unsafe extern "C" {
    fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, w_param: usize, l_param: isize) -> Lresult;
    fn GetMessageW(msg: *mut Msg, hwnd: Hwnd, filter_min: u32, filter_max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> Lresult;
    fn PostThreadMessageW(thread_id: u32, msg: u32, w_param: usize, l_param: isize) -> i32;
    fn GetCurrentThreadId() -> u32;
}

#[link(name = "wtsapi32")]
unsafe extern "C" {
    fn WTSRegisterSessionNotification(hwnd: Hwnd, flags: u32) -> i32;
    fn WTSUnRegisterSessionNotification(hwnd: Hwnd) -> i32;
}

unsafe extern "C" {
    fn RegisterSuspendResumeNotification(recipient: *mut c_void, flags: u32) -> *mut c_void;
    fn UnregisterSuspendResumeNotification(handle: *mut c_void) -> i32;
}

/// The one callback, set in start() before the thread
/// spawns; start() runs once per process (lib.rs setup).
static ON_LOCK: OnceLock<Arc<dyn Fn(LockReason) + Send + Sync>> = OnceLock::new();

fn fire(reason: LockReason) {
    if let Some(cb) = ON_LOCK.get() {
        cb(reason);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, w_param: usize, l_param: isize) -> Lresult {
    match msg {
        WM_WTSSESSION_CHANGE if w_param == WTS_SESSION_LOCK => {
            fire(LockReason::ScreenLocked);
            0
        }
        WM_POWERBROADCAST if w_param == PBT_APMSUSPEND => {
            fire(LockReason::Sleep);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) },
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct WindowsLockEvents {
    thread_id: Arc<AtomicU32>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Default for WindowsLockEvents {
    fn default() -> Self {
        WindowsLockEvents { thread_id: Arc::new(AtomicU32::new(0)), thread: None }
    }
}

impl LockEvents for WindowsLockEvents {
    fn start(
        &mut self,
        on_lock: Box<dyn Fn(LockReason) + Send + Sync>,
    ) -> Result<(), String> {
        let _ = ON_LOCK.set(Arc::from(on_lock));
        let thread_id = Arc::clone(&self.thread_id);
        self.thread = Some(std::thread::spawn(move || unsafe {
            thread_id.store(GetCurrentThreadId(), Ordering::SeqCst);
            let class_name = wide("SoirvuLockWindow");
            let instance = GetModuleHandleW(std::ptr::null());
            let class = WndClassW {
                style: 0,
                lpfn_wnd_proc: Some(wnd_proc),
                cb_cls_extra: 0,
                cb_wnd_extra: 0,
                h_instance: instance,
                h_icon: std::ptr::null_mut(),
                h_cursor: std::ptr::null_mut(),
                hbr_background: std::ptr::null_mut(),
                lpsz_menu_name: std::ptr::null(),
                lpsz_class_name: class_name.as_ptr(),
            };
            if RegisterClassW(&class) == 0 {
                return;
            }
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            );
            if hwnd.is_null() {
                return;
            }
            let wts_ok = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) != 0;
            let suspend = RegisterSuspendResumeNotification(hwnd, DEVICE_NOTIFY_WINDOW_HANDLE);
            if !wts_ok && suspend.is_null() {
                eprintln!("windows lock events: no session or suspend notifications (idle lock only)");
            }
            let mut msg = Msg {
                hwnd: std::ptr::null_mut(),
                message: 0,
                w_param: 0,
                l_param: 0,
                time: 0,
                pt: Point { x: 0, y: 0 },
            };
            loop {
                let got = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                if got <= 0 {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if wts_ok {
                WTSUnRegisterSessionNotification(hwnd);
            }
            if !suspend.is_null() {
                UnregisterSuspendResumeNotification(suspend);
            }
        }));
        Ok(())
    }
}

impl Drop for WindowsLockEvents {
    fn drop(&mut self) {
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                PostThreadMessageW(tid, WM_QUIT, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
