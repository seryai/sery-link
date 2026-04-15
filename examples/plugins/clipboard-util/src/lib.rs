//! Clipboard Utility Plugin
//!
//! Provides clipboard operations:
//! - Read from clipboard
//! - Write to clipboard
//! - Transform clipboard content (uppercase, lowercase, reverse)
//! - Get clipboard length

#![no_std]

extern crate alloc;
use alloc::string::String;
use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;

// Simple allocator implementation
struct DummyAllocator;

unsafe impl GlobalAlloc for DummyAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: DummyAllocator = DummyAllocator;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

static mut CLIPBOARD_BUFFER: [u8; 4096] = [0; 4096];
static mut CLIPBOARD_LEN: usize = 0;

// Host function imports (provided by the runtime)
extern "C" {
    fn get_clipboard(output_ptr: i32, output_max_len: i32) -> i32;
    fn set_clipboard(text_ptr: i32, text_len: i32) -> i32;
}

/// Read clipboard content into internal buffer
/// Returns bytes read or -1 on error
#[no_mangle]
pub extern "C" fn read_clipboard() -> i32 {
    unsafe {
        let bytes_read = get_clipboard(
            CLIPBOARD_BUFFER.as_mut_ptr() as i32,
            CLIPBOARD_BUFFER.len() as i32,
        );

        if bytes_read > 0 {
            CLIPBOARD_LEN = bytes_read as usize;
        }

        bytes_read
    }
}

/// Write text to clipboard
/// text_ptr: pointer to text in WASM memory
/// text_len: length of text
/// Returns bytes written or -1 on error
#[no_mangle]
pub extern "C" fn write_clipboard(text_ptr: i32, text_len: i32) -> i32 {
    unsafe { set_clipboard(text_ptr, text_len) }
}

/// Transform clipboard content to uppercase and write back
/// Returns bytes written or -1 on error
#[no_mangle]
pub extern "C" fn to_uppercase() -> i32 {
    unsafe {
        if CLIPBOARD_LEN == 0 {
            return -1;
        }

        // Convert to uppercase
        for i in 0..CLIPBOARD_LEN {
            let byte = CLIPBOARD_BUFFER[i];
            if byte >= b'a' && byte <= b'z' {
                CLIPBOARD_BUFFER[i] = byte - 32; // Convert to uppercase
            }
        }

        // Write back to clipboard
        set_clipboard(CLIPBOARD_BUFFER.as_ptr() as i32, CLIPBOARD_LEN as i32)
    }
}

/// Transform clipboard content to lowercase and write back
/// Returns bytes written or -1 on error
#[no_mangle]
pub extern "C" fn to_lowercase() -> i32 {
    unsafe {
        if CLIPBOARD_LEN == 0 {
            return -1;
        }

        // Convert to lowercase
        for i in 0..CLIPBOARD_LEN {
            let byte = CLIPBOARD_BUFFER[i];
            if byte >= b'A' && byte <= b'Z' {
                CLIPBOARD_BUFFER[i] = byte + 32; // Convert to lowercase
            }
        }

        // Write back to clipboard
        set_clipboard(CLIPBOARD_BUFFER.as_ptr() as i32, CLIPBOARD_LEN as i32)
    }
}

/// Reverse clipboard content and write back
/// Returns bytes written or -1 on error
#[no_mangle]
pub extern "C" fn reverse_text() -> i32 {
    unsafe {
        if CLIPBOARD_LEN == 0 {
            return -1;
        }

        // Reverse the buffer
        let mut i = 0;
        let mut j = CLIPBOARD_LEN - 1;
        while i < j {
            let temp = CLIPBOARD_BUFFER[i];
            CLIPBOARD_BUFFER[i] = CLIPBOARD_BUFFER[j];
            CLIPBOARD_BUFFER[j] = temp;
            i += 1;
            j -= 1;
        }

        // Write back to clipboard
        set_clipboard(CLIPBOARD_BUFFER.as_ptr() as i32, CLIPBOARD_LEN as i32)
    }
}

/// Get the length of clipboard content in buffer
#[no_mangle]
pub extern "C" fn get_clipboard_length() -> i32 {
    unsafe { CLIPBOARD_LEN as i32 }
}

/// Get plugin version (100 = v1.0.0)
#[no_mangle]
pub extern "C" fn get_version() -> i32 {
    100
}
