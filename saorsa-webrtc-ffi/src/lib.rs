//! FFI bindings for mobile and desktop applications

#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod types;

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::ffi::c_char;
use std::sync::Arc;
use std::sync::Mutex;
pub use types::{c_char_to_string, string_to_c_char, CallState, SaorsaResult};

/// Global runtime for async operations
#[allow(dead_code)]
static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_or_else(
            |_| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok()
                    .unwrap_or_else(|| {
                        // Last resort: create minimal runtime
                        tokio::runtime::Runtime::new()
                            .ok()
                            .unwrap_or_else(|| std::process::abort())
                    })
            },
            |rt| rt,
        )
});

/// Global handle storage
static HANDLES: Lazy<Mutex<HashMap<usize, Arc<SaorsaHandle>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Handle counter
static HANDLE_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(1));

/// Internal handle structure
struct SaorsaHandle {
    #[allow(dead_code)]
    identity: String,
    // In a full implementation, this would contain WebRTC service, call manager, etc.
}

impl SaorsaHandle {
    fn new(identity: String) -> Self {
        Self { identity }
    }
}

/// Initialize the library with an identity
///
/// # Safety
/// `identity` must be a valid null-terminated C string
/// Returns a handle pointer, or null on error
#[no_mangle]
pub extern "C" fn saorsa_init(identity: *const c_char) -> *mut std::ffi::c_void {
    // Validate input
    let identity_str = match unsafe { c_char_to_string(identity) } {
        Some(s) if !s.is_empty() => s,
        _ => return std::ptr::null_mut(),
    };

    // Create handle
    let handle = Arc::new(SaorsaHandle::new(identity_str));

    // Get next handle ID
    let handle_id = match HANDLE_COUNTER.lock() {
        Ok(mut counter) => {
            let id = *counter;
            *counter = counter.wrapping_add(1);
            id
        }
        Err(_) => return std::ptr::null_mut(),
    };

    // Store handle
    match HANDLES.lock() {
        Ok(mut handles) => {
            handles.insert(handle_id, handle);
            handle_id as *mut std::ffi::c_void
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Start a call to a peer
///
/// # Safety
/// `handle` must be a valid handle from `saorsa_init`
/// `peer` must be a valid null-terminated C string
/// Returns a call ID as a C string (caller must free), or null on error
#[no_mangle]
pub extern "C" fn saorsa_call(handle: *mut std::ffi::c_void, peer: *const c_char) -> *mut c_char {
    // Validate inputs
    if handle.is_null() {
        return std::ptr::null_mut();
    }

    let peer_str = match unsafe { c_char_to_string(peer) } {
        Some(s) if !s.is_empty() => s,
        _ => return std::ptr::null_mut(),
    };

    let handle_id = handle as usize;

    // Get handle
    let _handle = match HANDLES.lock() {
        Ok(handles) => match handles.get(&handle_id) {
            Some(h) => Arc::clone(h),
            None => return std::ptr::null_mut(),
        },
        Err(_) => return std::ptr::null_mut(),
    };

    // In a full implementation, would initiate actual call
    // For now, return a mock call ID
    let call_id = format!("call-{}-{}", handle_id, peer_str);
    unsafe { string_to_c_char(call_id) }
}

/// Get the current state of a call
///
/// # Safety
/// `handle` must be a valid handle from `saorsa_init`
/// `call_id` must be a valid null-terminated C string from `saorsa_call`
#[no_mangle]
pub extern "C" fn saorsa_call_state(
    handle: *mut std::ffi::c_void,
    _call_id: *const c_char,
) -> CallState {
    if handle.is_null() {
        return CallState::Failed;
    }

    // In a full implementation, would look up actual call state
    CallState::Active
}

/// End a call
///
/// # Safety
/// `handle` must be a valid handle from `saorsa_init`
/// `call_id` must be a valid null-terminated C string from `saorsa_call`
#[no_mangle]
pub extern "C" fn saorsa_end_call(
    handle: *mut std::ffi::c_void,
    _call_id: *const c_char,
) -> SaorsaResult {
    if handle.is_null() {
        return SaorsaResult::InvalidParameter;
    }

    // In a full implementation, would end the actual call
    SaorsaResult::Success
}

/// Free a string returned by the library
///
/// # Safety
/// `str_ptr` must be a string previously returned by this library
/// After calling this, `str_ptr` is invalid and must not be used
#[no_mangle]
pub extern "C" fn saorsa_free_string(str_ptr: *mut c_char) {
    if !str_ptr.is_null() {
        unsafe {
            let _ = std::ffi::CString::from_raw(str_ptr);
        }
    }
}

/// Free library resources
///
/// # Safety
/// `handle` must be a valid handle from `saorsa_init`
/// After calling this, `handle` is invalid and must not be used
#[no_mangle]
pub extern "C" fn saorsa_free(handle: *mut std::ffi::c_void) {
    if handle.is_null() {
        return;
    }

    let handle_id = handle as usize;

    // Remove handle
    if let Ok(mut handles) = HANDLES.lock() {
        handles.remove(&handle_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_with_valid_identity() {
        let identity = std::ffi::CString::new("test-identity")
            .ok()
            .map(|s| s.into_raw());
        if let Some(id_ptr) = identity {
            let handle = saorsa_init(id_ptr);
            assert!(!handle.is_null());

            saorsa_free(handle);
            unsafe {
                let _ = std::ffi::CString::from_raw(id_ptr);
            }
        }
    }

    #[test]
    fn test_init_with_null_identity() {
        let handle = saorsa_init(std::ptr::null());
        assert!(handle.is_null());
    }

    #[test]
    fn test_call_with_valid_handle() {
        let identity = std::ffi::CString::new("alice").ok().map(|s| s.into_raw());
        if let Some(id_ptr) = identity {
            let handle = saorsa_init(id_ptr);
            assert!(!handle.is_null());

            let peer = std::ffi::CString::new("bob").ok().map(|s| s.into_raw());
            if let Some(peer_ptr) = peer {
                let call_id = saorsa_call(handle, peer_ptr);
                assert!(!call_id.is_null());

                saorsa_free_string(call_id);
                unsafe {
                    let _ = std::ffi::CString::from_raw(peer_ptr);
                }
            }

            saorsa_free(handle);
            unsafe {
                let _ = std::ffi::CString::from_raw(id_ptr);
            }
        }
    }

    #[test]
    fn test_call_with_null_handle() {
        let peer = std::ffi::CString::new("bob").ok().map(|s| s.into_raw());
        if let Some(peer_ptr) = peer {
            let call_id = saorsa_call(std::ptr::null_mut(), peer_ptr);
            assert!(call_id.is_null());
            unsafe {
                let _ = std::ffi::CString::from_raw(peer_ptr);
            }
        }
    }

    #[test]
    fn test_call_state() {
        let identity = std::ffi::CString::new("alice").ok().map(|s| s.into_raw());
        if let Some(id_ptr) = identity {
            let handle = saorsa_init(id_ptr);

            let peer = std::ffi::CString::new("bob").ok().map(|s| s.into_raw());
            if let Some(peer_ptr) = peer {
                let call_id = saorsa_call(handle, peer_ptr);

                let state = saorsa_call_state(handle, call_id);
                assert_eq!(state, CallState::Active);

                saorsa_free_string(call_id);
                unsafe {
                    let _ = std::ffi::CString::from_raw(peer_ptr);
                }
            }

            saorsa_free(handle);
            unsafe {
                let _ = std::ffi::CString::from_raw(id_ptr);
            }
        }
    }

    #[test]
    fn test_end_call() {
        let identity = std::ffi::CString::new("alice").ok().map(|s| s.into_raw());
        if let Some(id_ptr) = identity {
            let handle = saorsa_init(id_ptr);

            let peer = std::ffi::CString::new("bob").ok().map(|s| s.into_raw());
            if let Some(peer_ptr) = peer {
                let call_id = saorsa_call(handle, peer_ptr);

                let result = saorsa_end_call(handle, call_id);
                assert_eq!(result, SaorsaResult::Success);

                saorsa_free_string(call_id);
                unsafe {
                    let _ = std::ffi::CString::from_raw(peer_ptr);
                }
            }

            saorsa_free(handle);
            unsafe {
                let _ = std::ffi::CString::from_raw(id_ptr);
            }
        }
    }

    #[test]
    fn test_double_free_is_safe() {
        let identity = std::ffi::CString::new("test").ok().map(|s| s.into_raw());
        if let Some(id_ptr) = identity {
            let handle = saorsa_init(id_ptr);
            saorsa_free(handle);
            saorsa_free(handle); // Should not crash
            unsafe {
                let _ = std::ffi::CString::from_raw(id_ptr);
            }
        }
    }
}
