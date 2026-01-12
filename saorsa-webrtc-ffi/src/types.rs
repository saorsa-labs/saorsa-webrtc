//! FFI type definitions

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[cfg(test)]
use std::os::raw::c_int;

/// FFI result code
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaorsaResult {
    /// Operation succeeded
    Success = 0,
    /// Invalid parameter
    InvalidParameter = 1,
    /// Out of memory
    OutOfMemory = 2,
    /// Not initialized
    NotInitialized = 3,
    /// Already initialized
    AlreadyInitialized = 4,
    /// Connection failed
    ConnectionFailed = 5,
    /// Internal error
    InternalError = 99,
}

/// FFI call state
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallState {
    /// Call is being initiated
    Connecting = 0,
    /// Call is active
    Active = 1,
    /// Call is ended
    Ended = 2,
    /// Call failed
    Failed = 3,
}

/// Convert Rust string to C string (caller must free)
///
/// # Safety
/// The returned pointer must be freed with saorsa_free_string or CString::from_raw
pub unsafe fn string_to_c_char(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Convert C string to Rust string
///
/// # Safety
/// `ptr` must be a valid null-terminated C string or null
pub unsafe fn c_char_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_codes() {
        assert_eq!(SaorsaResult::Success as c_int, 0);
        assert_eq!(SaorsaResult::InvalidParameter as c_int, 1);
        assert_eq!(SaorsaResult::InternalError as c_int, 99);
    }

    #[test]
    fn test_call_states() {
        assert_eq!(CallState::Connecting as c_int, 0);
        assert_eq!(CallState::Active as c_int, 1);
        assert_eq!(CallState::Ended as c_int, 2);
        assert_eq!(CallState::Failed as c_int, 3);
    }

    #[test]
    fn test_string_to_c_char() {
        let test_str = "hello world".to_string();
        let c_ptr = unsafe { string_to_c_char(test_str.clone()) };

        assert!(!c_ptr.is_null());

        // Convert back to verify
        let recovered = unsafe { c_char_to_string(c_ptr) };
        assert_eq!(recovered, Some(test_str));

        // Clean up
        unsafe {
            if !c_ptr.is_null() {
                let _ = CString::from_raw(c_ptr);
            }
        }
    }

    #[test]
    fn test_c_char_to_string_null() {
        let result = unsafe { c_char_to_string(std::ptr::null()) };
        assert_eq!(result, None);
    }

    #[test]
    fn test_string_roundtrip() {
        let strings = vec![
            "simple",
            "with-dashes",
            "with_underscores",
            "alice-bob-charlie-david",
        ];

        for s in strings {
            let c_ptr = unsafe { string_to_c_char(s.to_string()) };
            assert!(!c_ptr.is_null());

            let recovered = unsafe { c_char_to_string(c_ptr) };
            assert_eq!(recovered, Some(s.to_string()));

            unsafe {
                if !c_ptr.is_null() {
                    let _ = CString::from_raw(c_ptr);
                }
            }
        }
    }
}
