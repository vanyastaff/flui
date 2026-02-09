//! Build progress tracking and reporting with visual indicators.
//!
//! This module provides unified progress reporting for all build phases
//! across different platforms (Android, Web, Desktop).

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::Arc;
use std::time::Duration;

/// Build phase indicator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildPhase {
    /// Validating environment and tools
    Validate,
    /// Building Rust libraries
    BuildRust,
    /// Building platform-specific artifacts (APK, WASM, etc.)
    BuildPlatform,
    /// Cleaning build artifacts
    Clean,
}

impl BuildPhase {
    /// Returns the display name for this phase
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            BuildPhase::Validate => "Validate",
            BuildPhase::BuildRust => "Build Rust",
            BuildPhase::BuildPlatform => "Build Platform",
            BuildPhase::Clean => "Clean",
        }
    }

    /// Returns the emoji for this phase
    #[must_use]
    pub fn emoji(&self) -> &'static str {
        match self {
            BuildPhase::Validate => "🔍",
            BuildPhase::BuildRust => "⚙️",
            BuildPhase::BuildPlatform => "📦",
            BuildPhase::Clean => "🧹",
        }
    }
}

/// Progress reporter for a single build
#[derive(Debug)]
pub struct BuildProgress {
    multi: Arc<MultiProgress>,
    main_bar: ProgressBar,
    phase_bar: Option<ProgressBar>,
    #[allow(dead_code)]
    platform: String,
}

impl BuildProgress {
    /// Create a new build progress reporter
    ///
    /// # Arguments
    ///
    /// * `platform` - Platform name (e.g., "Android", "Web", "Desktop")
    /// * `multi` - Shared multi-progress for coordinating multiple builds
    pub fn new(platform: impl Into<String>, multi: Arc<MultiProgress>) -> Self {
        let platform = platform.into();

        let main_bar = multi.add(ProgressBar::new(100));
        main_bar.set_style(
            ProgressStyle::default_bar()
                .template("{prefix:.bold} [{bar:40.cyan/blue}] {pos}% {msg}")
                .unwrap()
                .progress_chars("█▓▒░ "),
        );
        main_bar.set_prefix(format!("Building {platform}"));

        Self {
            multi,
            main_bar,
            phase_bar: None,
            platform,
        }
    }

    /// Start a new build phase
    ///
    /// # Arguments
    ///
    /// * `phase` - The build phase to start
    /// * `message` - Optional status message
    pub fn start_phase(&mut self, phase: BuildPhase, message: Option<&str>) {
        // Remove previous phase bar if exists
        if let Some(bar) = self.phase_bar.take() {
            bar.finish_and_clear();
        }

        let phase_bar = self.multi.add(ProgressBar::new_spinner());
        phase_bar.set_style(
            ProgressStyle::default_spinner()
                .template("  {spinner:.green} {prefix:.bold} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        phase_bar.set_prefix(format!("{} {}", phase.emoji(), phase.display_name()));

        if let Some(msg) = message {
            phase_bar.set_message(msg.to_string());
        }

        phase_bar.enable_steady_tick(Duration::from_millis(80));
        self.phase_bar = Some(phase_bar);
    }

    /// Update the current phase message
    pub fn set_message(&self, message: impl Into<String>) {
        if let Some(bar) = &self.phase_bar {
            bar.set_message(message.into());
        }
    }

    /// Finish the current phase successfully
    pub fn finish_phase(&mut self, message: impl Into<String>) {
        if let Some(bar) = self.phase_bar.take() {
            bar.finish_with_message(format!("✓ {}", message.into()));
        }
    }

    /// Finish the current phase with error
    pub fn fail_phase(&mut self, message: impl Into<String>) {
        if let Some(bar) = self.phase_bar.take() {
            bar.abandon_with_message(format!("✗ {}", message.into()));
        }
    }

    /// Update overall build progress (0-100)
    pub fn set_progress(&self, percent: u8) {
        self.main_bar.set_position(u64::from(percent));
    }

    /// Finish the entire build successfully
    pub fn finish(&self, message: impl Into<String>) {
        self.main_bar.set_position(100);
        self.main_bar
            .finish_with_message(format!("✓ {}", message.into()));

        if let Some(bar) = &self.phase_bar {
            bar.finish_and_clear();
        }
    }

    /// Finish the build with error
    pub fn fail(&self, message: impl Into<String>) {
        self.main_bar
            .abandon_with_message(format!("✗ {}", message.into()));

        if let Some(bar) = &self.phase_bar {
            bar.finish_and_clear();
        }
    }
}

impl Drop for BuildProgress {
    fn drop(&mut self) {
        // Ensure bars are cleaned up
        if let Some(bar) = self.phase_bar.take() {
            bar.finish_and_clear();
        }
    }
}

/// Global progress manager for coordinating multiple platform builds
#[derive(Debug)]
pub struct ProgressManager {
    multi: Arc<MultiProgress>,
}

impl ProgressManager {
    /// Create a new progress manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            multi: Arc::new(MultiProgress::new()),
        }
    }

    /// Create a progress reporter for a platform build
    pub fn create_build(&self, platform: impl Into<String>) -> BuildProgress {
        BuildProgress::new(platform, self.multi.clone())
    }

    /// Wait for all progress bars to finish
    pub fn join(&self) {
        // MultiProgress will automatically clean up when all bars are done
    }
}

impl Default for ProgressManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_phase_display() {
        assert_eq!(BuildPhase::Validate.display_name(), "Validate");
        assert_eq!(BuildPhase::BuildRust.display_name(), "Build Rust");
        assert_eq!(BuildPhase::BuildPlatform.display_name(), "Build Platform");
        assert_eq!(BuildPhase::Clean.display_name(), "Clean");
    }

    #[test]
    fn test_build_phase_emoji() {
        assert_eq!(BuildPhase::Validate.emoji(), "🔍");
        assert_eq!(BuildPhase::BuildRust.emoji(), "⚙️");
        assert_eq!(BuildPhase::BuildPlatform.emoji(), "📦");
        assert_eq!(BuildPhase::Clean.emoji(), "🧹");
    }
}
