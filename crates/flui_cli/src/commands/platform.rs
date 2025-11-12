use anyhow::Result;
use console::style;

pub fn add(platforms: Vec<String>) -> Result<()> {
    println!("{}", style(format!("Adding platform support: {}", platforms.join(", "))).green().bold());
    println!();
    println!("{}", style("Note: Platform management not yet fully implemented").yellow());
    println!("  This will be available in a future version");
    println!();
    println!("  For now, platform directories should be added manually:");
    println!("  • platforms/android/");
    println!("  • platforms/ios/");
    println!("  • platforms/web/");

    Ok(())
}

pub fn remove(platform: String) -> Result<()> {
    println!("{}", style(format!("Removing platform support: {}", platform)).green().bold());
    println!();
    println!("{}", style("Note: Platform management not yet fully implemented").yellow());
    println!("  This will be available in a future version");

    Ok(())
}

pub fn list() -> Result<()> {
    println!("{}", style("Supported platforms:").green().bold());
    println!();
    println!("  {} Android", style("✓").green());
    println!("  {} iOS (macOS only)", style("✓").green());
    println!("  {} Web (WASM)", style("✓").green());
    println!("  {} Windows", style("✓").green());
    println!("  {} Linux", style("✓").green());
    println!("  {} macOS", style("✓").green());

    Ok(())
}
