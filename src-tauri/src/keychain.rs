//! The device secret's home in the OS keychain (Phase 8).
//!
//! One item per vault: a random 32-byte secret, stored in the
//! data-protection keychain under a biometric access control
//! (the design's device slot, §5). The keychain does the
//! fingerprint check; this module only ever sees the bytes
//! after the user passes it.
//!
//! Availability is a property of the BUILD, not the machine:
//! without a `keychain-access-groups` entitlement in the
//! running app's signature (a development profile — see spike
//! 6), the calls return Unavailable and the UI hides Touch ID
//! entirely, offering a note instead. The passphrase and
//! recovery slots never depend on any of this.

/// Why a keychain operation did not produce a secret.
#[derive(Debug)]
// The stub implementation (any build that is not macOS with
// the touch-id feature) only ever constructs Unavailable,
// so the allowance is scoped to exactly those builds — in
// the real build every variant must stay reachable, and the
// warning should fire if one stops being produced.
#[cfg_attr(
    not(all(target_os = "macos", feature = "touch-id")),
    allow(dead_code)
)]
pub enum KeychainError {
    /// This build cannot use the data-protection keychain:
    /// built without the touch-id feature, not macOS, or
    /// signed without the keychain entitlement.
    Unavailable,
    /// The user dismissed the fingerprint prompt.
    Cancelled,
    /// No item for this vault (never enrolled, or the item
    /// was invalidated when biometry changed).
    NotFound,
    /// The Security framework said no for a reason without
    /// its own variant above. The raw status is not carried:
    /// the three statuses that matter (-128, -25300, -34018)
    /// are the other variants, and callers only need to know
    /// this was none of them.
    Failed,
}

#[cfg(not(all(target_os = "macos", feature = "touch-id")))]
mod imp {
    use super::KeychainError;

    pub fn group() -> Option<String> {
        None
    }

    pub fn store(_vault_id: &str, _secret: &[u8]) -> Result<(), KeychainError> {
        Err(KeychainError::Unavailable)
    }

    pub fn read(_vault_id: &str) -> Result<Vec<u8>, KeychainError> {
        Err(KeychainError::Unavailable)
    }

    pub fn delete(_vault_id: &str) -> Result<(), KeychainError> {
        Err(KeychainError::Unavailable)
    }
}

#[cfg(all(target_os = "macos", feature = "touch-id"))]
mod imp {
    use super::KeychainError;
    use std::ffi::{c_char, c_void, CString};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFDictionaryRef = *const c_void;

    // The callback structs are used address-only: what the
    // framework reads is Apple's exported value, so the Rust
    // side declares them as an opaque type and takes the
    // address of the export.
    #[repr(C)]
    struct Callbacks {
        _opaque: [u8; 0],
    }

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecClass: CFStringRef;
        static kSecClassGenericPassword: CFStringRef;
        static kSecAttrService: CFStringRef;
        static kSecAttrAccount: CFStringRef;
        static kSecAttrAccessGroup: CFStringRef;
        static kSecAttrAccessControl: CFStringRef;
        static kSecAttrAccessibleWhenUnlockedThisDeviceOnly: CFStringRef;
        static kSecValueData: CFStringRef;
        static kSecReturnData: CFStringRef;
        static kSecUseDataProtectionKeychain: CFStringRef;
        static kSecUseOperationPrompt: CFStringRef;

        fn SecItemAdd(query: CFDictionaryRef, result: *mut CFTypeRef) -> i32;
        fn SecItemCopyMatching(query: CFDictionaryRef, result: *mut CFTypeRef) -> i32;
        fn SecItemDelete(query: CFDictionaryRef) -> i32;
        fn SecAccessControlCreateWithFlags(
            allocator: CFTypeRef,
            protection: CFStringRef,
            flags: u64,
            error: *mut CFTypeRef,
        ) -> CFTypeRef;
        fn SecTaskCreateFromSelf(allocator: CFTypeRef) -> CFTypeRef;
        fn SecTaskCopyValueForEntitlement(
            task: CFTypeRef,
            entitlement: CFStringRef,
            error: *mut CFTypeRef,
        ) -> CFTypeRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: CFTypeRef;
        static kCFTypeDictionaryKeyCallBacks: Callbacks;
        static kCFTypeDictionaryValueCallBacks: Callbacks;

        fn CFDictionaryCreate(
            allocator: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            count: isize,
            key_callbacks: *const Callbacks,
            value_callbacks: *const Callbacks,
        ) -> CFDictionaryRef;
        fn CFStringCreateWithCString(
            allocator: CFTypeRef,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFDataCreate(allocator: CFTypeRef, bytes: *const u8, length: isize) -> CFTypeRef;
        fn CFDataGetLength(data: CFTypeRef) -> isize;
        fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
        fn CFArrayGetCount(array: CFTypeRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
        fn CFStringGetCString(
            string: CFStringRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> bool;
        fn CFRelease(cf: CFTypeRef);
    }

    const UTF8: u32 = 0x0800_0100;
    const ERR_SUCCESS: i32 = 0;
    const ERR_USER_CANCELED: i32 = -128;
    const ERR_ITEM_NOT_FOUND: i32 = -25300;
    const ERR_MISSING_ENTITLEMENT: i32 = -34018;
    /// SecAccessControlCreateFlags: biometryCurrentSet.
    const BIOMETRY_CURRENT_SET: u64 = 1 << 3;

    // Debug builds use a sibling service, mirroring the
    // config-dir split in lib.rs: a dev build enrolling a
    // vault must not overwrite the release build's item (the
    // keychain keys items by service + account alone).
    const SERVICE: &str = if cfg!(debug_assertions) {
        "soirvu.device.dev"
    } else {
        "soirvu.device"
    };

    /// An owned CoreFoundation reference, released on drop.
    struct Owned(CFTypeRef);

    impl Owned {
        fn as_ref(&self) -> CFTypeRef {
            self.0
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn cf_string(text: &str) -> Owned {
        let c = CString::new(text).expect("static text has no NUL");
        Owned(unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) })
    }

    fn string_value(cf: CFStringRef) -> Option<String> {
        let mut buffer = [0i8; 256];
        let ok = unsafe { CFStringGetCString(cf, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) };
        if !ok {
            return None;
        }
        let cstr = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
        Some(cstr.to_string_lossy().into_owned())
    }

    fn dictionary(pairs: &[(CFStringRef, CFTypeRef)]) -> Owned {
        let keys: Vec<CFTypeRef> = pairs.iter().map(|(k, _)| *k as CFTypeRef).collect();
        let values: Vec<CFTypeRef> = pairs.iter().map(|(_, v)| *v).collect();
        Owned(unsafe {
            CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                pairs.len() as isize,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            )
        })
    }

    /// The running app's first keychain access group, read
    /// from its own entitlements. None = this build was not
    /// signed with the entitlement (the ad-hoc case), so
    /// Touch ID is unavailable in it.
    pub fn group() -> Option<String> {
        unsafe {
            let task = Owned(SecTaskCreateFromSelf(std::ptr::null()));
            if task.as_ref().is_null() {
                return None;
            }
            let key = cf_string("keychain-access-groups");
            let mut error: CFTypeRef = std::ptr::null();
            let value = Owned(SecTaskCopyValueForEntitlement(task.as_ref(), key.as_ref(), &mut error));
            if value.as_ref().is_null() || CFArrayGetCount(value.as_ref()) < 1 {
                return None;
            }
            string_value(CFArrayGetValueAtIndex(value.as_ref(), 0) as CFStringRef)
        }
    }

    /// The query's owned strings; they must outlive the
    /// SecItem call that uses their references.
    fn owned_strings(vault_id: &str, group_name: &str) -> Vec<Owned> {
        vec![cf_string(SERVICE), cf_string(vault_id), cf_string(group_name)]
    }

    fn status_error(status: i32) -> KeychainError {
        match status {
            ERR_USER_CANCELED => KeychainError::Cancelled,
            ERR_ITEM_NOT_FOUND => KeychainError::NotFound,
            ERR_MISSING_ENTITLEMENT => KeychainError::Unavailable,
            other => {
                // Not content, just the platform's number —
                // it is what tells -25293 (authentication
                // failed) apart from -25308 (interaction not
                // allowed) when Touch ID misbehaves.
                eprintln!("keychain: unmapped OSStatus {other}");
                KeychainError::Failed
            }
        }
    }

    pub fn store(vault_id: &str, secret: &[u8]) -> Result<(), KeychainError> {
        let group_name = group().ok_or(KeychainError::Unavailable)?;
        let owned = owned_strings(vault_id, &group_name);
        let (service, account, grp) = (owned[0].as_ref(), owned[1].as_ref(), owned[2].as_ref());
        unsafe {
            // Replace any previous item: re-enrollment mints a
            // fresh secret, and a duplicate add would fail.
            let del = dictionary(&[
                (kSecClass, kSecClassGenericPassword),
                (kSecAttrService, service),
                (kSecAttrAccount, account),
                (kSecAttrAccessGroup, grp),
                (kSecUseDataProtectionKeychain, kCFBooleanTrue),
            ]);
            let _ = SecItemDelete(del.as_ref());

            let mut acl_error: CFTypeRef = std::ptr::null();
            let acl = Owned(SecAccessControlCreateWithFlags(
                std::ptr::null(),
                kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                BIOMETRY_CURRENT_SET,
                &mut acl_error,
            ));
            if acl.as_ref().is_null() {
                return Err(KeychainError::Unavailable);
            }
            let data = Owned(CFDataCreate(std::ptr::null(), secret.as_ptr(), secret.len() as isize));
            let add = dictionary(&[
                (kSecClass, kSecClassGenericPassword),
                (kSecAttrService, service),
                (kSecAttrAccount, account),
                (kSecAttrAccessGroup, grp),
                (kSecUseDataProtectionKeychain, kCFBooleanTrue),
                (kSecAttrAccessControl, acl.as_ref()),
                (kSecValueData, data.as_ref()),
            ]);
            let status = SecItemAdd(add.as_ref(), std::ptr::null_mut());
            if status == ERR_SUCCESS {
                Ok(())
            } else {
                Err(status_error(status))
            }
        }
    }

    pub fn read(vault_id: &str) -> Result<Vec<u8>, KeychainError> {
        let group_name = group().ok_or(KeychainError::Unavailable)?;
        let owned = owned_strings(vault_id, &group_name);
        let (service, account, grp) = (owned[0].as_ref(), owned[1].as_ref(), owned[2].as_ref());
        // The deprecated-but-functional prompt key: a plain
        // string, no LAContext object to build over FFI. The
        // system still shows its own Touch ID sheet.
        let prompt = cf_string("Unlock Soirvu");
        unsafe {
            let query = dictionary(&[
                (kSecClass, kSecClassGenericPassword),
                (kSecAttrService, service),
                (kSecAttrAccount, account),
                (kSecAttrAccessGroup, grp),
                (kSecUseDataProtectionKeychain, kCFBooleanTrue),
                (kSecReturnData, kCFBooleanTrue),
                (kSecUseOperationPrompt, prompt.as_ref()),
            ]);
            let mut result: CFTypeRef = std::ptr::null();
            let status = SecItemCopyMatching(query.as_ref(), &mut result);
            if status != ERR_SUCCESS {
                return Err(status_error(status));
            }
            let data = Owned(result);
            let len = CFDataGetLength(data.as_ref());
            let ptr = CFDataGetBytePtr(data.as_ref());
            if ptr.is_null() || len < 0 {
                return Err(KeychainError::Failed);
            }
            Ok(std::slice::from_raw_parts(ptr, len as usize).to_vec())
        }
    }

    pub fn delete(vault_id: &str) -> Result<(), KeychainError> {
        let group_name = group().ok_or(KeychainError::Unavailable)?;
        let owned = owned_strings(vault_id, &group_name);
        let (service, account, grp) = (owned[0].as_ref(), owned[1].as_ref(), owned[2].as_ref());
        unsafe {
            let query = dictionary(&[
                (kSecClass, kSecClassGenericPassword),
                (kSecAttrService, service),
                (kSecAttrAccount, account),
                (kSecAttrAccessGroup, grp),
                (kSecUseDataProtectionKeychain, kCFBooleanTrue),
            ]);
            let status = SecItemDelete(query.as_ref());
            if status == ERR_SUCCESS || status == ERR_ITEM_NOT_FOUND {
                Ok(())
            } else {
                Err(status_error(status))
            }
        }
    }
}

/// Whether this build can use Touch ID at all (macOS with
/// the keychain-access-groups entitlement in its signature).
pub fn capable() -> bool {
    imp::group().is_some()
}

pub fn store(vault_id: &str, secret: &[u8]) -> Result<(), KeychainError> {
    imp::store(vault_id, secret)
}

pub fn read(vault_id: &str) -> Result<Vec<u8>, KeychainError> {
    imp::read(vault_id)
}

pub fn delete(vault_id: &str) -> Result<(), KeychainError> {
    imp::delete(vault_id)
}
