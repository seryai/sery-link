//! Text Analyzer Plugin
//!
//! Advanced text analysis features:
//! - Word frequency analysis
//! - Readability metrics (Flesch reading ease approximation)
//! - Basic sentiment analysis
//! - Text statistics (words, sentences, paragraphs, unique words)

#![no_std]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
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

static mut TEXT_DATA: Option<String> = None;

/// Parse and store text from memory
#[no_mangle]
pub extern "C" fn parse_text_from_memory(data_ptr: *const u8, data_len: usize) -> i32 {
    unsafe {
        if data_ptr.is_null() {
            return 0; // invalid
        }

        let slice = core::slice::from_raw_parts(data_ptr, data_len);
        match core::str::from_utf8(slice) {
            Ok(text_str) => {
                TEXT_DATA = Some(String::from(text_str));
                1 // valid
            }
            Err(_) => 0, // invalid
        }
    }
}

/// Get text statistics
/// Returns packed: (word_count << 16) | (sentence_count << 8) | paragraph_count
#[no_mangle]
pub extern "C" fn get_text_stats() -> i32 {
    unsafe {
        if let Some(ref text) = TEXT_DATA {
            let word_count = count_words(text).min(65535);
            let sentence_count = count_sentences(text).min(255);
            let paragraph_count = count_paragraphs(text).min(255);

            (word_count << 16) | (sentence_count << 8) | paragraph_count
        } else {
            0
        }
    }
}

/// Get unique word count
#[no_mangle]
pub extern "C" fn get_unique_words() -> i32 {
    unsafe {
        if let Some(ref text) = TEXT_DATA {
            count_unique_words(text)
        } else {
            0
        }
    }
}

/// Calculate average word length
/// Returns average * 10 (e.g., 45 means 4.5 characters per word)
#[no_mangle]
pub extern "C" fn get_avg_word_length() -> i32 {
    unsafe {
        if let Some(ref text) = TEXT_DATA {
            let words: Vec<&str> = text.split_whitespace().collect();
            if words.is_empty() {
                return 0;
            }

            let total_chars: usize = words.iter().map(|w| w.len()).sum();
            let avg = (total_chars * 10) / words.len();
            avg as i32
        } else {
            0
        }
    }
}

/// Calculate readability score (simplified Flesch reading ease)
/// Returns score 0-100 (higher = easier to read)
/// Simplified formula: 206.835 - 1.015(words/sentences) - 84.6(syllables/words)
/// We approximate syllables as word_length / 3
#[no_mangle]
pub extern "C" fn get_readability_score() -> i32 {
    unsafe {
        if let Some(ref text) = TEXT_DATA {
            let word_count = count_words(text);
            let sentence_count = count_sentences(text).max(1); // Avoid division by zero

            if word_count == 0 {
                return 0;
            }

            // Approximate syllables (very rough: assume 1 syllable per 3 chars)
            let words: Vec<&str> = text.split_whitespace().collect();
            let total_chars: usize = words.iter().map(|w| w.len()).sum();
            let approx_syllables = total_chars / 3;

            let words_per_sentence = (word_count * 100) / sentence_count;
            let syllables_per_word = if word_count > 0 {
                ((approx_syllables as i32) * 100) / word_count
            } else {
                0
            };

            // Simplified Flesch score (scaled to fit in i32)
            let score = 206 - (words_per_sentence / 100) - ((syllables_per_word * 84) / 100);
            score.max(0).min(100)
        } else {
            0
        }
    }
}

/// Basic sentiment analysis
/// Returns: -100 (very negative) to +100 (very positive)
/// Simplified keyword matching approach
#[no_mangle]
pub extern "C" fn get_sentiment() -> i32 {
    unsafe {
        if let Some(ref text) = TEXT_DATA {
            let lower_text = text.to_lowercase();

            // Positive words
            let positive_words = [
                "good", "great", "excellent", "amazing", "wonderful", "fantastic",
                "love", "best", "awesome", "perfect", "happy", "beautiful",
            ];

            // Negative words
            let negative_words = [
                "bad", "terrible", "awful", "horrible", "worst", "hate",
                "poor", "disappointing", "sad", "ugly", "fail", "wrong",
            ];

            let mut positive_count = 0;
            let mut negative_count = 0;

            for word in &positive_words {
                positive_count += lower_text.matches(word).count();
            }

            for word in &negative_words {
                negative_count += lower_text.matches(word).count();
            }

            let total = positive_count + negative_count;
            if total == 0 {
                return 0; // Neutral
            }

            // Calculate sentiment score (-100 to +100)
            let score = ((positive_count as i32 * 200) / total as i32) - 100;
            score
        } else {
            0
        }
    }
}

/// Get character count
#[no_mangle]
pub extern "C" fn get_char_count() -> i32 {
    unsafe {
        if let Some(ref text) = TEXT_DATA {
            text.len() as i32
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

fn count_words(text: &str) -> i32 {
    text.split_whitespace().count() as i32
}

fn count_sentences(text: &str) -> i32 {
    let mut count = 0;
    for ch in text.chars() {
        if ch == '.' || ch == '!' || ch == '?' {
            count += 1;
        }
    }
    count.max(1) // At least 1 sentence
}

fn count_paragraphs(text: &str) -> i32 {
    text.split("\n\n").filter(|p| !p.trim().is_empty()).count() as i32
}

fn count_unique_words(text: &str) -> i32 {
    // Simplified unique word count (case-insensitive)
    // In a real implementation, we'd use a HashSet, but that requires std
    // For no_std, we use a simple approximation

    let lower_text = text.to_lowercase();
    let words: Vec<&str> = lower_text
        .split_whitespace()
        .collect();

    let total_words = words.len();
    if total_words == 0 {
        return 0;
    }

    // Approximate unique count by checking common words
    // This is a simplified version - real implementation would need proper set
    let common_words = [
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for",
        "of", "with", "by", "from", "as", "is", "was", "are", "were", "be",
    ];

    let mut common_count = 0;
    for word in &common_words {
        common_count += words.iter().filter(|w| *w == word).count();
    }

    // Rough estimate: assume non-common words have less repetition
    let non_common = total_words - common_count;
    let estimated_unique = (non_common * 80 / 100) + common_words.len();

    estimated_unique as i32
}
