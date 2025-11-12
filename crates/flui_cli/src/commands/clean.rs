use anyhow::{Context, Result};
use console::style;
use std::process::Command;

pub fn execute(deep: bool, platform: Option<String>) -> Result<()> {
    println!("{}", style("Cleaning build artifacts...").green().bold());
    println!();

    if deep {
        println!("  {} Deep clean enabled", style("→").cyan());
    }

    if let Some(platform) = platform {
        println!("  {} Cleaning platform: {}", style("→").cyan(), style(&platform).cyan());
        clean_platform(&platform)?;
    } else {
        // Clean using cargo
        let mut cmd = Command::new("cargo");
        cmd.arg("clean");

        let status = cmd.status().context("Failed to run cargo clean")?;

        if !status.success() {
            anyhow::bail!("Clean failed");
        }

        // Clean platform-specific directories
        if deep {
            clean_platform_dirs()?;
        }
    }

    println!();
    println!("{}", style("✓ Clean completed").green().bold());

    Ok(())
}

fn clean_platform(platform: &str) -> Result<()> {
    let platform_dir = std::path::Path::new("platforms").join(platform);

    if platform_dir.exists() {
        match platform {
            "android" => {
                let build_dir = platform_dir.join("app").join("build");
                if build_dir.exists() {
                    std::fs::remove_dir_all(&build_dir)?;
                    println!("  {} Removed {}", style("✓").green(), build_dir.display());
                }

                let gradle_dir = platform_dir.join(".gradle");
                if gradle_dir.exists() {
                    std::fs::remove_dir_all(&gradle_dir)?;
                    println!("  {} Removed {}", style("✓").green(), gradle_dir.display());
                }
            }
            "web" => {
                let pkg_dir = platform_dir.join("pkg");
                if pkg_dir.exists() {
                    std::fs::remove_dir_all(&pkg_dir)?;
                    println!("  {} Removed {}", style("✓").green(), pkg_dir.display());
                }
            }
            "ios" => {
                let build_dir = platform_dir.join("build");
                if build_dir.exists() {
                    std::fs::remove_dir_all(&build_dir)?;
                    println!("  {} Removed {}", style("✓").green(), build_dir.display());
                }
            }
            _ => {
                println!("  {} Unknown platform: {}", style("!").yellow(), platform);
            }
        }
    }

    Ok(())
}

fn clean_platform_dirs() -> Result<()> {
    let platforms = ["android", "ios", "web"];

    for platform in &platforms {
        let _ = clean_platform(platform);
    }

    Ok(())
}
