//! CSV Parser Plugin - Production example for Sery Link
//!
//! This plugin demonstrates data-source capability by parsing CSV files.
//! It analyzes CSV structure and returns column/row counts.

#![no_std]

// Panic handler required for no_std
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// Test CSV data embedded in the plugin
/// In a real implementation, this would come from read_file host function
const TEST_CSV: &str = "name,age,city\nAlice,30,NYC\nBob,25,SF\nCarol,35,LA";

/// Parse CSV and return number of columns
///
/// This demonstrates CSV parsing capability.
/// Returns the number of columns detected in the CSV header.
#[no_mangle]
pub extern "C" fn get_column_count() -> i32 {
    // Count commas in first line + 1
    let first_line = TEST_CSV.split('\n').next().unwrap_or("");
    let comma_count = first_line.bytes().filter(|&b| b == b',').count();
    (comma_count + 1) as i32
}

/// Parse CSV and return number of rows (excluding header)
///
/// Returns the number of data rows in the CSV.
#[no_mangle]
pub extern "C" fn get_row_count() -> i32 {
    let line_count = TEST_CSV.split('\n').count();
    // Subtract 1 for header
    if line_count > 0 {
        (line_count - 1) as i32
    } else {
        0
    }
}

/// Validate CSV format
///
/// Returns 1 if CSV is valid, 0 if invalid.
/// Checks that all rows have the same number of columns.
#[no_mangle]
pub extern "C" fn validate_csv() -> i32 {
    let lines: [&str; 16] = {
        let mut arr = [""; 16];
        let mut i = 0;
        for line in TEST_CSV.split('\n') {
            if i < 16 {
                arr[i] = line;
                i += 1;
            }
        }
        arr
    };

    if lines[0].is_empty() {
        return 0; // Invalid - no header
    }

    // Count columns in header
    let header_cols = lines[0].bytes().filter(|&b| b == b',').count() + 1;

    // Check each data row has same number of columns
    for line in &lines[1..] {
        if line.is_empty() {
            break;
        }
        let cols = line.bytes().filter(|&b| b == b',').count() + 1;
        if cols != header_cols {
            return 0; // Invalid - inconsistent column count
        }
    }

    1 // Valid
}

/// Initialize the plugin
#[no_mangle]
pub extern "C" fn _initialize() {
    // Initialization logic
}

/// Get plugin version (encoded as integer: MAJOR * 1000 + MINOR)
#[no_mangle]
pub extern "C" fn get_version() -> i32 {
    1000 // Version 1.0
}
