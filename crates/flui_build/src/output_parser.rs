//! Output parsers for different build tools.
//!
//! This module provides parsers that extract meaningful progress information
//! from the output of various build tools (cargo, gradle, wasm-pack).

use std::sync::Arc;

/// Parsed build event from tool output
#[derive(Debug, Clone)]
pub enum BuildEvent {
    /// Tool started a new task
    Started {
        /// Task name
        task: String,
    },
    /// Progress update for current task
    Progress {
        /// Current progress value
        current: usize,
        /// Total progress value
        total: usize,
    },
    /// Task completed successfully
    Completed {
        /// Task name
        task: String,
        /// Duration in milliseconds
        duration_ms: Option<u64>,
    },
    /// Warning message
    Warning {
        /// Warning message text
        message: String,
    },
    /// Error message
    Error {
        /// Error message text
        message: String,
    },
    /// Generic info message
    Info {
        /// Info message text
        message: String,
    },
}

/// Trait for parsing build tool output
pub trait OutputParser: Send + Sync {
    /// Parse a line of output and extract build event if any
    fn parse_line(&self, line: &str) -> Option<BuildEvent>;

    /// Get the tool name
    fn tool_name(&self) -> &str;
}

/// Parser for cargo output
#[derive(Debug)]
pub struct CargoParser;

impl OutputParser for CargoParser {
    fn parse_line(&self, line: &str) -> Option<BuildEvent> {
        let line = line.trim();

        // Cargo format: "   Compiling crate_name v1.0.0"
        if line.starts_with("Compiling") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                return Some(BuildEvent::Started {
                    task: format!("Compiling {}", parts[1]),
                });
            }
        }

        // "    Finished release [optimized] target(s) in 12.34s"
        if line.starts_with("Finished") {
            if let Some(time_str) = line.split(" in ").nth(1) {
                let time_str = time_str.trim_end_matches('s');
                if let Ok(seconds) = time_str.parse::<f64>() {
                    return Some(BuildEvent::Completed {
                        task: "Rust compilation".to_string(),
                        duration_ms: Some((seconds * 1000.0) as u64),
                    });
                }
            }
        }

        // "warning: ..."
        if line.starts_with("warning:") {
            return Some(BuildEvent::Warning {
                message: line.trim_start_matches("warning:").trim().to_string(),
            });
        }

        // "error: ..." or "error[E0xxx]: ..."
        if line.starts_with("error:") || line.starts_with("error[") {
            return Some(BuildEvent::Error {
                message: line.to_string(),
            });
        }

        None
    }

    fn tool_name(&self) -> &str {
        "cargo"
    }
}

/// Parser for Gradle output
#[derive(Debug)]
pub struct GradleParser;

impl OutputParser for GradleParser {
    fn parse_line(&self, line: &str) -> Option<BuildEvent> {
        let line = line.trim();

        // Gradle format: "> Task :app:compileDebugKotlin"
        if line.starts_with("> Task") {
            let task_name = line
                .trim_start_matches("> Task")
                .trim()
                .split(':')
                .last()?
                .to_string();
            return Some(BuildEvent::Started { task: task_name });
        }

        // "BUILD SUCCESSFUL in 12s"
        if line.starts_with("BUILD SUCCESSFUL") {
            if let Some(time_str) = line.split(" in ").nth(1) {
                let time_str = time_str.trim_end_matches('s');
                if let Ok(seconds) = time_str.parse::<u64>() {
                    return Some(BuildEvent::Completed {
                        task: "Gradle build".to_string(),
                        duration_ms: Some(seconds * 1000),
                    });
                }
            }
        }

        // "BUILD FAILED in 5s"
        if line.starts_with("BUILD FAILED") {
            return Some(BuildEvent::Error {
                message: "Gradle build failed".to_string(),
            });
        }

        // "w: warning message"
        if line.starts_with("w:") {
            return Some(BuildEvent::Warning {
                message: line.trim_start_matches("w:").trim().to_string(),
            });
        }

        // "e: error message"
        if line.starts_with("e:") {
            return Some(BuildEvent::Error {
                message: line.trim_start_matches("e:").trim().to_string(),
            });
        }

        None
    }

    fn tool_name(&self) -> &str {
        "gradle"
    }
}

/// Parser for wasm-pack output
#[derive(Debug)]
pub struct WasmPackParser;

impl OutputParser for WasmPackParser {
    fn parse_line(&self, line: &str) -> Option<BuildEvent> {
        let line = line.trim();

        // wasm-pack format: "[INFO]: 📦  Compiling to WebAssembly..."
        if line.contains("[INFO]:") {
            let message = line.split("[INFO]:").nth(1)?.trim();
            // Remove emoji if present
            let message = message
                .chars()
                .skip_while(|c| !c.is_alphanumeric())
                .collect::<String>()
                .trim()
                .to_string();

            if message.contains("Compiling") {
                return Some(BuildEvent::Started {
                    task: message.clone(),
                });
            }

            return Some(BuildEvent::Info { message });
        }

        // "[WARN]: ..."
        if line.contains("[WARN]:") {
            return Some(BuildEvent::Warning {
                message: line.split("[WARN]:").nth(1)?.trim().to_string(),
            });
        }

        // "✨  Done in 12.34s"
        if line.contains("Done in") {
            if let Some(time_str) = line.split("Done in ").nth(1) {
                let time_str = time_str.trim_end_matches('s');
                if let Ok(seconds) = time_str.parse::<f64>() {
                    return Some(BuildEvent::Completed {
                        task: "WASM build".to_string(),
                        duration_ms: Some((seconds * 1000.0) as u64),
                    });
                }
            }
        }

        None
    }

    fn tool_name(&self) -> &str {
        "wasm-pack"
    }
}

/// Get parser for a specific tool
pub fn get_parser(tool: &str) -> Arc<dyn OutputParser> {
    match tool.to_lowercase().as_str() {
        "cargo" | "cargo-ndk" => Arc::new(CargoParser),
        "gradle" | "gradlew" | "gradlew.bat" => Arc::new(GradleParser),
        "wasm-pack" => Arc::new(WasmPackParser),
        "xcodebuild" | "xcode" => Arc::new(XcodeParser),
        _ => Arc::new(CargoParser), // Default to cargo parser
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_parser_compiling() {
        let parser = CargoParser;
        let event = parser.parse_line("   Compiling flui_build v0.1.0");
        assert!(matches!(event, Some(BuildEvent::Started { .. })));
    }

    #[test]
    fn test_cargo_parser_finished() {
        let parser = CargoParser;
        let event = parser.parse_line("    Finished release [optimized] target(s) in 12.34s");
        assert!(matches!(
            event,
            Some(BuildEvent::Completed {
                duration_ms: Some(12340),
                ..
            })
        ));
    }

    #[test]
    fn test_cargo_parser_warning() {
        let parser = CargoParser;
        let event = parser.parse_line("warning: unused variable");
        assert!(matches!(event, Some(BuildEvent::Warning { .. })));
    }

    #[test]
    fn test_gradle_parser_task() {
        let parser = GradleParser;
        let event = parser.parse_line("> Task :app:compileDebugKotlin");
        assert!(matches!(event, Some(BuildEvent::Started { .. })));
    }

    #[test]
    fn test_gradle_parser_success() {
        let parser = GradleParser;
        let event = parser.parse_line("BUILD SUCCESSFUL in 12s");
        assert!(matches!(
            event,
            Some(BuildEvent::Completed {
                duration_ms: Some(12000),
                ..
            })
        ));
    }

    #[test]
    fn test_wasm_pack_parser_info() {
        let parser = WasmPackParser;
        let event = parser.parse_line("[INFO]: 📦  Compiling to WebAssembly...");
        assert!(matches!(event, Some(BuildEvent::Started { .. })));
    }
}

/// Parser for Xcode/xcodebuild output
#[derive(Debug)]
pub struct XcodeParser;

impl OutputParser for XcodeParser {
    fn parse_line(&self, line: &str) -> Option<BuildEvent> {
        let line = line.trim();

        // Xcode format: "=== BUILD TARGET xxx OF PROJECT yyy WITH CONFIGURATION Debug ==="
        if line.starts_with("=== BUILD TARGET") {
            if let Some(target) = line.split("TARGET ").nth(1) {
                let target_name = target.split(" OF ").next()?.to_string();
                return Some(BuildEvent::Started {
                    task: format!("Building {}", target_name),
                });
            }
        }

        // "▸ Compiling Foo.swift"
        if line.starts_with("▸ Compiling") {
            let file = line.trim_start_matches("▸ Compiling").trim();
            return Some(BuildEvent::Info {
                message: format!("Compiling {}", file),
            });
        }

        // "▸ Linking libFlui.dylib"
        if line.starts_with("▸ Linking") {
            let file = line.trim_start_matches("▸ Linking").trim();
            return Some(BuildEvent::Info {
                message: format!("Linking {}", file),
            });
        }

        // "▸ Building library for iOS"
        if line.starts_with("▸ Building") {
            let msg = line.trim_start_matches("▸ Building").trim();
            return Some(BuildEvent::Started {
                task: format!("Building {}", msg),
            });
        }

        // "** BUILD SUCCEEDED ** [12.5 sec]"
        if line.contains("BUILD SUCCEEDED") {
            if let Some(time_str) = line.split('[').nth(1) {
                let time_str = time_str.trim_end_matches(']').trim_end_matches(" sec");
                if let Ok(seconds) = time_str.parse::<f64>() {
                    return Some(BuildEvent::Completed {
                        task: "Xcode build".to_string(),
                        duration_ms: Some((seconds * 1000.0) as u64),
                    });
                }
            }
        }

        // "** BUILD FAILED **"
        if line.contains("BUILD FAILED") {
            return Some(BuildEvent::Error {
                message: "Xcode build failed".to_string(),
            });
        }

        // "warning: ..."
        if line.contains("warning:") {
            return Some(BuildEvent::Warning {
                message: line.split("warning:").nth(1)?.trim().to_string(),
            });
        }

        // "error: ..."
        if line.contains("error:") {
            return Some(BuildEvent::Error {
                message: line.split("error:").nth(1)?.trim().to_string(),
            });
        }

        None
    }

    fn tool_name(&self) -> &str {
        "xcodebuild"
    }
}
