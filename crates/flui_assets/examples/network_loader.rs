//! Network loader example for flui-assets.
//!
//! This example demonstrates loading assets from HTTP/HTTPS URLs.
//!
//! Run with: `cargo run --example network_loader --features network`

#[cfg(feature = "network")]
use flui_assets::NetworkLoader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "network")]
    {
        println!("=== FLUI Assets Network Loader Example ===\n");

        let loader = NetworkLoader::new();

        // Example 1: Load data from a public API
        println!("1. Loading Random Bytes from httpbin.org");
        println!("------------------------------------------");

        match loader.load_url("https://httpbin.org/bytes/100").await {
            Ok(bytes) => {
                println!("  ✓ Successfully loaded {} bytes", bytes.len());
                println!("  ✓ First 10 bytes: {:?}", &bytes[..10.min(bytes.len())]);
            }
            Err(e) => {
                println!("  ✗ Failed to load: {}", e);
                println!("  ℹ This might be a network connectivity issue");
            }
        }
        println!();

        // Example 2: Load text data
        println!("2. Loading Text Data");
        println!("--------------------");

        match loader.load_text("https://httpbin.org/robots.txt").await {
            Ok(text) => {
                println!("  ✓ Successfully loaded text");
                let lines: Vec<&str> = text.lines().take(3).collect();
                println!("  ✓ First 3 lines:");
                for line in lines {
                    println!("    {}", line);
                }
            }
            Err(e) => {
                println!("  ✗ Failed to load: {}", e);
            }
        }
        println!();

        // Example 3: Demonstrate error handling
        println!("3. Error Handling (Invalid URL)");
        println!("--------------------------------");

        match loader
            .load_url("https://this-domain-does-not-exist-12345.com/data")
            .await
        {
            Ok(_) => {
                println!("  ✗ Unexpected success");
            }
            Err(e) => {
                println!("  ✓ Correctly handled error");
                println!("  ℹ Error: {}", e);
            }
        }
        println!();

        // Example 4: Using NetworkLoader with AssetRegistry
        println!("4. Integration with AssetRegistry");
        println!("----------------------------------");
        println!("  ℹ NetworkLoader can be used with custom Asset implementations");
        println!("  ℹ See the Asset trait documentation for details");
        println!();

        println!("=== Example Complete ===");
        println!("\nNote: These examples require internet connectivity.");
        println!("If any requests failed, check your network connection.");

        Ok(())
    }

    #[cfg(not(feature = "network"))]
    {
        eprintln!("Error: This example requires the 'network' feature.");
        eprintln!("Run with: cargo run --example network_loader --features network");
        std::process::exit(1);
    }
}
