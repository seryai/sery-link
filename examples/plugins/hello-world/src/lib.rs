//! Hello World Plugin - Minimal no_std WASM plugin for Sery Link
//!
//! This is a minimal example plugin that demonstrates the plugin system.
//! It exposes simple functions that can be called by the host.

#![no_std]

// Panic handler required for no_std
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// Returns a greeting code
///
/// This function is exported to the WASM module and can be called by the host.
/// Returns 42 as a success code.
#[no_mangle]
pub extern "C" fn greet() -> i32 {
    42
}

/// Parse function for data-source capability
///
/// In a real data-source plugin, this would:
/// 1. Read file bytes from WASM memory
/// 2. Parse the file format (e.g., custom CSV, XML, JSON)
/// 3. Return parsed data as JSON
///
/// For this example, it just returns 0 (success).
#[no_mangle]
pub extern "C" fn parse() -> i32 {
    0
}

/// Initialize the plugin
///
/// Called when the plugin is loaded into the runtime.
#[no_mangle]
pub extern "C" fn _initialize() {
    // Initialization logic would go here
}
