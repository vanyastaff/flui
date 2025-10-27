//! Debug Flags Demo
//!
//! This example demonstrates how to use debug flags for development and debugging.

use flui_core::{DebugFlags, debug_println, debug_exec};

fn main() {
    println!("=== DebugFlags Demo ===\n");

    // Start with all flags disabled
    DebugFlags::disable_all();
    println!("1. All flags disabled");
    print_current_flags();

    // Enable specific flags
    println!("\n2. Enabling PRINT_BUILD_SCOPE and PRINT_LAYOUT...");
    DebugFlags::enable(DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_LAYOUT);
    print_current_flags();

    // Test flag checking
    println!("\n3. Testing flag checks:");
    if DebugFlags::is_enabled(DebugFlags::PRINT_BUILD_SCOPE) {
        println!("   ✓ PRINT_BUILD_SCOPE is enabled");
    }
    if !DebugFlags::is_enabled(DebugFlags::PRINT_MARK_NEEDS_BUILD) {
        println!("   ✗ PRINT_MARK_NEEDS_BUILD is disabled");
    }

    // Use debug_println! macro
    println!("\n4. Using debug_println! macro:");
    debug_println!(PRINT_BUILD_SCOPE, "   This message appears because PRINT_BUILD_SCOPE is enabled");
    debug_println!(PRINT_MARK_NEEDS_BUILD, "   This message will NOT appear (flag disabled)");

    // Use debug_exec! macro
    println!("\n5. Using debug_exec! macro:");
    debug_exec!(PRINT_LAYOUT, {
        println!("   Executing debug code block (PRINT_LAYOUT enabled)");
        let result = 42 + 58;
        println!("   Computed result: {}", result);
    });

    debug_exec!(CHECK_ELEMENT_LIFECYCLE, {
        println!("   This block will NOT execute (flag disabled)");
    });

    // Enable all flags
    println!("\n6. Enabling ALL flags:");
    DebugFlags::enable_all();
    print_current_flags();

    // Disable specific flag
    println!("\n7. Disabling PRINT_BUILD_SCOPE:");
    DebugFlags::disable(DebugFlags::PRINT_BUILD_SCOPE);
    print_current_flags();

    // Set specific combination
    println!("\n8. Setting exact flag combination:");
    DebugFlags::set_global(
        DebugFlags::PRINT_LAYOUT |
        DebugFlags::CHECK_ELEMENT_LIFECYCLE |
        DebugFlags::PRINT_DEPENDENCIES
    );
    print_current_flags();

    // Clean up
    println!("\n9. Disabling all flags:");
    DebugFlags::disable_all();
    print_current_flags();

    println!("\n=== Demo Complete ===");
}

fn print_current_flags() {
    let flags = DebugFlags::get_global();
    println!("   Current flags: {:?}", flags);
    println!("   Bits: 0b{:09b} ({})", flags.bits(), flags.bits());
}
