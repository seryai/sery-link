//! JSON Transformer Plugin
//!
//! Provides JSON manipulation functions:
//! - Pretty-print JSON
//! - Minify JSON
//! - Validate JSON
//! - Extract keys

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

static mut JSON_DATA: Option<String> = None;
static mut LAST_ERROR: Option<String> = None;

/// Parse and store JSON from memory
#[no_mangle]
pub extern "C" fn parse_json_from_memory(data_ptr: *const u8, data_len: usize) -> i32 {
    unsafe {
        if data_ptr.is_null() {
            LAST_ERROR = Some(String::from("null pointer"));
            return 0; // invalid
        }

        let slice = core::slice::from_raw_parts(data_ptr, data_len);
        match core::str::from_utf8(slice) {
            Ok(json_str) => {
                // Basic JSON validation - check for balanced braces/brackets
                if is_valid_json(json_str) {
                    JSON_DATA = Some(String::from(json_str));
                    1 // valid
                } else {
                    LAST_ERROR = Some(String::from("invalid JSON"));
                    0 // invalid
                }
            }
            Err(_) => {
                LAST_ERROR = Some(String::from("invalid UTF-8"));
                0 // invalid
            }
        }
    }
}

/// Pretty-print JSON (add indentation)
/// Returns pointer to formatted JSON string
#[no_mangle]
pub extern "C" fn pretty_print() -> *const u8 {
    unsafe {
        if let Some(ref json) = JSON_DATA {
            let pretty = format_json(json, true);
            JSON_DATA = Some(pretty.clone());
            pretty.as_ptr()
        } else {
            core::ptr::null()
        }
    }
}

/// Minify JSON (remove whitespace)
/// Returns pointer to minified JSON string
#[no_mangle]
pub extern "C" fn minify() -> *const u8 {
    unsafe {
        if let Some(ref json) = JSON_DATA {
            let minified = format_json(json, false);
            JSON_DATA = Some(minified.clone());
            minified.as_ptr()
        } else {
            core::ptr::null()
        }
    }
}

/// Validate JSON structure
/// Returns 1 if valid, 0 if invalid
#[no_mangle]
pub extern "C" fn validate() -> i32 {
    unsafe {
        if let Some(ref json) = JSON_DATA {
            if is_valid_json(json) {
                1
            } else {
                0
            }
        } else {
            0
        }
    }
}

/// Get the length of stored JSON
#[no_mangle]
pub extern "C" fn get_json_length() -> i32 {
    unsafe {
        if let Some(ref json) = JSON_DATA {
            json.len() as i32
        } else {
            0
        }
    }
}

/// Get plugin version (100 = v1.0.0)
#[no_mangle]
pub extern "C" fn get_version() -> i32 {
    100
}

// Helper functions

fn is_valid_json(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }

    // Basic validation: check for balanced braces and brackets
    let mut brace_count = 0i32;
    let mut bracket_count = 0i32;
    let mut in_string = false;
    let mut escape_next = false;

    for ch in s.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => brace_count += 1,
            '}' if !in_string => brace_count -= 1,
            '[' if !in_string => bracket_count += 1,
            ']' if !in_string => bracket_count -= 1,
            _ => {}
        }

        // Early exit if counts go negative
        if brace_count < 0 || bracket_count < 0 {
            return false;
        }
    }

    // Must start with { or [
    let first_char = s.chars().next().unwrap();
    let valid_start = first_char == '{' || first_char == '[';

    valid_start && brace_count == 0 && bracket_count == 0 && !in_string
}

fn format_json(json: &str, pretty: bool) -> String {
    if pretty {
        // Add indentation
        let mut result = String::new();
        let mut indent_level = 0usize;
        let mut in_string = false;
        let mut escape_next = false;

        for ch in json.trim().chars() {
            if escape_next {
                result.push(ch);
                escape_next = false;
                continue;
            }

            match ch {
                '\\' if in_string => {
                    result.push(ch);
                    escape_next = true;
                }
                '"' => {
                    result.push(ch);
                    in_string = !in_string;
                }
                '{' | '[' if !in_string => {
                    result.push(ch);
                    result.push('\n');
                    indent_level += 1;
                    add_indent(&mut result, indent_level);
                }
                '}' | ']' if !in_string => {
                    result.push('\n');
                    indent_level = indent_level.saturating_sub(1);
                    add_indent(&mut result, indent_level);
                    result.push(ch);
                }
                ',' if !in_string => {
                    result.push(ch);
                    result.push('\n');
                    add_indent(&mut result, indent_level);
                }
                ':' if !in_string => {
                    result.push(ch);
                    result.push(' ');
                }
                ' ' | '\n' | '\r' | '\t' if !in_string => {
                    // Skip whitespace outside strings
                }
                _ => {
                    result.push(ch);
                }
            }
        }
        result
    } else {
        // Minify - remove all whitespace outside strings
        let mut result = String::new();
        let mut in_string = false;
        let mut escape_next = false;

        for ch in json.chars() {
            if escape_next {
                result.push(ch);
                escape_next = false;
                continue;
            }

            match ch {
                '\\' if in_string => {
                    result.push(ch);
                    escape_next = true;
                }
                '"' => {
                    result.push(ch);
                    in_string = !in_string;
                }
                ' ' | '\n' | '\r' | '\t' if !in_string => {
                    // Skip whitespace outside strings
                }
                _ => {
                    result.push(ch);
                }
            }
        }
        result
    }
}

fn add_indent(s: &mut String, level: usize) {
    for _ in 0..level {
        s.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_json() {
        assert!(is_valid_json(r#"{"key": "value"}"#));
        assert!(is_valid_json(r#"[1, 2, 3]"#));
        assert!(!is_valid_json(r#"{"key": "value""#)); // missing }
        assert!(!is_valid_json(r#"not json"#));
    }

    #[test]
    fn test_minify() {
        let input = r#"{
            "name": "test",
            "value": 123
        }"#;
        let expected = r#"{"name":"test","value":123}"#;
        assert_eq!(format_json(input, false), expected);
    }

    #[test]
    fn test_pretty_print() {
        let input = r#"{"name":"test","value":123}"#;
        let output = format_json(input, true);
        assert!(output.contains('\n'));
        assert!(output.contains("  ")); // indentation
    }
}
