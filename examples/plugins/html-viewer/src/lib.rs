//! HTML Viewer Plugin
//!
//! Provides HTML analysis functions:
//! - Extract text content
//! - Count tags
//! - Validate structure
//! - Get metadata

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

static mut HTML_DATA: Option<String> = None;

/// Parse and store HTML from memory
#[no_mangle]
pub extern "C" fn parse_html_from_memory(data_ptr: *const u8, data_len: usize) -> i32 {
    unsafe {
        if data_ptr.is_null() {
            return 0; // invalid
        }

        let slice = core::slice::from_raw_parts(data_ptr, data_len);
        match core::str::from_utf8(slice) {
            Ok(html_str) => {
                HTML_DATA = Some(String::from(html_str));
                1 // valid
            }
            Err(_) => 0, // invalid
        }
    }
}

/// Extract plain text from HTML (strip tags)
/// Returns packed: (has_content << 16) | (word_count << 8) | paragraph_count
#[no_mangle]
pub extern "C" fn extract_text() -> i32 {
    unsafe {
        if let Some(ref html) = HTML_DATA {
            let text = strip_tags(html);
            let has_content = if text.len() > 0 { 1 } else { 0 };
            let word_count = count_words(&text).min(255);
            let paragraph_count = count_paragraphs(html).min(255);

            (has_content << 16) | (word_count << 8) | paragraph_count
        } else {
            0
        }
    }
}

/// Count HTML tags
/// Returns packed: (total_tags << 16) | (unique_tags << 8) | depth
#[no_mangle]
pub extern "C" fn count_tags() -> i32 {
    unsafe {
        if let Some(ref html) = HTML_DATA {
            let total_tags = count_all_tags(html).min(65535);
            let unique_tags = count_unique_tags(html).min(255);
            let depth = calculate_depth(html).min(255);

            (total_tags << 16) | (unique_tags << 8) | depth
        } else {
            0
        }
    }
}

/// Validate HTML structure
/// Returns 1 if valid (balanced tags), 0 if invalid
#[no_mangle]
pub extern "C" fn validate_structure() -> i32 {
    unsafe {
        if let Some(ref html) = HTML_DATA {
            if is_balanced(html) { 1 } else { 0 }
        } else {
            0
        }
    }
}

/// Get character count of HTML
#[no_mangle]
pub extern "C" fn get_html_length() -> i32 {
    unsafe {
        if let Some(ref html) = HTML_DATA {
            html.len() as i32
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

fn strip_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;

    let mut chars = html.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            in_tag = true;

            // Check for script/style tags
            let remaining: String = chars.clone().take(10).collect();
            if remaining.to_lowercase().starts_with("script") {
                in_script = true;
            } else if remaining.to_lowercase().starts_with("/script") {
                in_script = false;
            } else if remaining.to_lowercase().starts_with("style") {
                in_style = true;
            } else if remaining.to_lowercase().starts_with("/style") {
                in_style = false;
            }
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag && !in_script && !in_style {
            result.push(ch);
        }
    }

    result
}

fn count_words(text: &str) -> i32 {
    text.split_whitespace().count() as i32
}

fn count_paragraphs(html: &str) -> i32 {
    html.matches("<p").count() as i32 + html.matches("<P").count() as i32
}

fn count_all_tags(html: &str) -> i32 {
    html.matches('<').count() as i32
}

fn count_unique_tags(html: &str) -> i32 {
    // Simplified: count common unique tags
    let mut count = 0;
    let tags = ["html", "head", "body", "div", "span", "p", "a", "img", "h1", "h2", "h3"];

    for tag in &tags {
        if html.contains(tag) {
            count += 1;
        }
    }
    count
}

fn calculate_depth(html: &str) -> i32 {
    let mut max_depth: i32 = 0;
    let mut current_depth: i32 = 0;

    let mut in_tag = false;
    let mut tag_name = String::new();

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_name.clear();
            }
            '>' => {
                in_tag = false;
                if !tag_name.is_empty() {
                    if tag_name.starts_with('/') {
                        current_depth = current_depth.saturating_sub(1);
                    } else if !tag_name.contains('/') && !is_self_closing(&tag_name) {
                        current_depth += 1;
                        if current_depth > max_depth {
                            max_depth = current_depth;
                        }
                    }
                }
            }
            _ if in_tag => {
                if ch != ' ' && ch != '=' && ch != '"' && ch != '\'' {
                    tag_name.push(ch);
                }
            }
            _ => {}
        }
    }

    max_depth
}

fn is_self_closing(tag: &str) -> bool {
    matches!(tag.to_lowercase().as_str(), "br" | "hr" | "img" | "input" | "meta" | "link")
}

fn is_balanced(html: &str) -> bool {
    let mut stack_depth: i32 = 0;

    let mut in_tag = false;
    let mut tag_name = String::new();

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_name.clear();
            }
            '>' => {
                in_tag = false;
                if !tag_name.is_empty() {
                    if tag_name.starts_with('/') {
                        stack_depth = stack_depth.saturating_sub(1);
                    } else if !tag_name.contains('/') && !is_self_closing(&tag_name) {
                        stack_depth += 1;
                    }
                }
            }
            _ if in_tag => {
                if ch != ' ' && ch != '=' && ch != '"' && ch != '\'' {
                    tag_name.push(ch);
                }
            }
            _ => {}
        }

        if stack_depth < 0 {
            return false;
        }
    }

    stack_depth == 0
}
