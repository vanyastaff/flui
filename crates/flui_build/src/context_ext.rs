/// Extension trait for BuilderContext with convenient utility methods.
///
/// This module provides the `BuilderContextExt` trait which adds utility methods
/// to `BuilderContext` without bloating the core struct. This follows the
/// Extension Trait pattern from Rust API Guidelines.
///
/// # Examples
///
/// ```rust
/// use flui_build::*;
/// use std::path::PathBuf;
///
/// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
///     .with_platform(Platform::Android {
///         targets: vec!["aarch64-linux-android".to_string()],
///     })
///     .with_profile(Profile::Release)
///     .with_feature("webgpu".to_string())
///     .build();
///
/// // Extension methods are available automatically
/// assert!(ctx.is_release());
/// assert!(ctx.has_feature("webgpu"));
/// assert!(!ctx.is_debug());
/// ```
use std::path::PathBuf;

use crate::platform::{BuilderContext, Platform, Profile};

/// Extension trait providing convenient methods for BuilderContext.
///
/// This trait is automatically implemented for all `BuilderContext` instances
/// via blanket implementation, providing useful utility methods without
/// modifying the core struct.
///
/// # Examples
///
/// ## Profile Checks
///
/// ```rust
/// use flui_build::*;
/// use std::path::PathBuf;
///
/// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
///     .with_platform(Platform::Desktop { target: None })
///     .with_profile(Profile::Release)
///     .build();
///
/// if ctx.is_release() {
///     println!("Optimized build");
/// }
/// ```
///
/// ## Cargo Arguments
///
/// ```rust
/// use flui_build::*;
/// use std::path::PathBuf;
///
/// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
///     .with_platform(Platform::Android {
///         targets: vec!["aarch64-linux-android".to_string()],
///     })
///     .with_profile(Profile::Release)
///     .with_feature("webgpu".to_string())
///     .build();
///
/// let args = ctx.cargo_args();
/// // Returns: ["--release", "--features", "webgpu"]
/// ```
///
/// ## Feature Checks
///
/// ```rust
/// use flui_build::*;
/// use std::path::PathBuf;
///
/// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
///     .with_platform(Platform::Web { target: "web".to_string() })
///     .with_profile(Profile::Debug)
///     .with_feature("webgpu".to_string())
///     .with_feature("audio".to_string())
///     .build();
///
/// if ctx.has_feature("webgpu") {
///     println!("WebGPU enabled");
/// }
///
/// if ctx.has_any_feature(&["audio", "video"]) {
///     println!("Media features enabled");
/// }
/// ```
pub trait BuilderContextExt {
    /// Check if this is a release build.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Desktop { target: None })
    ///     .with_profile(Profile::Release)
    ///     .build();
    ///
    /// assert!(ctx.is_release());
    /// ```
    fn is_release(&self) -> bool;

    /// Check if this is a debug build.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Desktop { target: None })
    ///     .with_profile(Profile::Debug)
    ///     .build();
    ///
    /// assert!(ctx.is_debug());
    /// ```
    fn is_debug(&self) -> bool;

    /// Get cargo arguments for this build configuration.
    ///
    /// Returns command-line arguments that can be passed to `cargo` based on
    /// the profile and enabled features.
    ///
    /// # Returns
    ///
    /// A vector of arguments:
    /// - `["--release"]` for release builds
    /// - `[]` for debug builds
    /// - `["--features", "feature1", "--features", "feature2"]` for enabled features
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Desktop { target: None })
    ///     .with_profile(Profile::Release)
    ///     .with_feature("webgpu".to_string())
    ///     .build();
    ///
    /// let args = ctx.cargo_args();
    /// assert!(args.contains(&"--release".to_string()));
    /// assert!(args.contains(&"--features".to_string()));
    /// assert!(args.contains(&"webgpu".to_string()));
    /// ```
    fn cargo_args(&self) -> Vec<String>;

    /// Get the platform-specific output directory.
    ///
    /// Appends the platform name to the base output directory.
    ///
    /// # Returns
    ///
    /// Path like `output_dir/android`, `output_dir/web`, or `output_dir/desktop`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Android {
    ///         targets: vec!["aarch64-linux-android".to_string()],
    ///     })
    ///     .with_profile(Profile::Release)
    ///     .with_output_dir(PathBuf::from("out"))
    ///     .build();
    ///
    /// let platform_dir = ctx.platform_output_dir();
    /// assert!(platform_dir.ends_with("android"));
    /// ```
    fn platform_output_dir(&self) -> PathBuf;

    /// Check if a specific feature is enabled.
    ///
    /// # Arguments
    ///
    /// * `feature` - Feature name to check
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Web { target: "web".to_string() })
    ///     .with_profile(Profile::Debug)
    ///     .with_feature("webgpu".to_string())
    ///     .build();
    ///
    /// assert!(ctx.has_feature("webgpu"));
    /// assert!(!ctx.has_feature("vulkan"));
    /// ```
    fn has_feature(&self, feature: &str) -> bool;

    /// Check if any of the given features are enabled.
    ///
    /// Returns `true` if at least one feature from the list is enabled.
    ///
    /// # Arguments
    ///
    /// * `features` - Slice of feature names to check
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Desktop { target: None })
    ///     .with_profile(Profile::Debug)
    ///     .with_feature("audio".to_string())
    ///     .build();
    ///
    /// assert!(ctx.has_any_feature(&["audio", "video"]));
    /// assert!(!ctx.has_any_feature(&["video", "image"]));
    /// ```
    fn has_any_feature(&self, features: &[&str]) -> bool;

    /// Check if all given features are enabled.
    ///
    /// Returns `true` only if all features from the list are enabled.
    ///
    /// # Arguments
    ///
    /// * `features` - Slice of feature names to check
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Desktop { target: None })
    ///     .with_profile(Profile::Debug)
    ///     .with_feature("audio".to_string())
    ///     .with_feature("video".to_string())
    ///     .build();
    ///
    /// assert!(ctx.has_all_features(&["audio", "video"]));
    /// assert!(!ctx.has_all_features(&["audio", "video", "image"]));
    /// ```
    fn has_all_features(&self, features: &[&str]) -> bool;

    /// Get the number of enabled features.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Desktop { target: None })
    ///     .with_profile(Profile::Debug)
    ///     .with_feature("audio".to_string())
    ///     .with_feature("video".to_string())
    ///     .build();
    ///
    /// assert_eq!(ctx.feature_count(), 2);
    /// ```
    fn feature_count(&self) -> usize;

    /// Check if this build is for Android.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Android {
    ///         targets: vec!["aarch64-linux-android".to_string()],
    ///     })
    ///     .with_profile(Profile::Debug)
    ///     .build();
    ///
    /// assert!(ctx.is_android());
    /// assert!(!ctx.is_web());
    /// ```
    fn is_android(&self) -> bool;

    /// Check if this build is for Web/WASM.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Web { target: "web".to_string() })
    ///     .with_profile(Profile::Debug)
    ///     .build();
    ///
    /// assert!(ctx.is_web());
    /// assert!(!ctx.is_desktop());
    /// ```
    fn is_web(&self) -> bool;

    /// Check if this build is for Desktop.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Desktop { target: None })
    ///     .with_profile(Profile::Debug)
    ///     .build();
    ///
    /// assert!(ctx.is_desktop());
    /// assert!(!ctx.is_android());
    /// ```
    fn is_desktop(&self) -> bool;
}

// Blanket implementation for BuilderContext
impl BuilderContextExt for BuilderContext {
    fn is_release(&self) -> bool {
        matches!(self.profile, Profile::Release)
    }

    fn is_debug(&self) -> bool {
        matches!(self.profile, Profile::Debug)
    }

    fn cargo_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        // Add profile flag
        if let Some(flag) = self.profile.cargo_flag() {
            args.push(flag.to_string());
        }

        // Add features
        for feature in &self.features {
            args.push("--features".to_string());
            args.push(feature.clone());
        }

        args
    }

    fn platform_output_dir(&self) -> PathBuf {
        self.output_dir.join(self.platform.name())
    }

    fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }

    fn has_any_feature(&self, features: &[&str]) -> bool {
        features.iter().any(|f| self.has_feature(f))
    }

    fn has_all_features(&self, features: &[&str]) -> bool {
        features.iter().all(|f| self.has_feature(f))
    }

    fn feature_count(&self) -> usize {
        self.features.len()
    }

    fn is_android(&self) -> bool {
        matches!(self.platform, Platform::Android { .. })
    }

    fn is_web(&self) -> bool {
        matches!(self.platform, Platform::Web { .. })
    }

    fn is_desktop(&self) -> bool {
        matches!(self.platform, Platform::Desktop { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuilderContextBuilder;

    fn create_context(profile: Profile, features: Vec<String>, platform: Platform) -> BuilderContext {
        let mut builder = BuilderContextBuilder::new(PathBuf::from("."))
            .with_platform(platform)
            .with_profile(profile);

        for feature in features {
            builder = builder.with_feature(feature);
        }

        builder.build()
    }

    #[test]
    fn test_is_release() {
        let ctx = create_context(Profile::Release, vec![], Platform::Desktop { target: None });
        assert!(ctx.is_release());
        assert!(!ctx.is_debug());
    }

    #[test]
    fn test_is_debug() {
        let ctx = create_context(Profile::Debug, vec![], Platform::Desktop { target: None });
        assert!(ctx.is_debug());
        assert!(!ctx.is_release());
    }

    #[test]
    fn test_cargo_args_release() {
        let ctx = create_context(Profile::Release, vec![], Platform::Desktop { target: None });
        let args = ctx.cargo_args();
        assert!(args.contains(&"--release".to_string()));
    }

    #[test]
    fn test_cargo_args_with_features() {
        let ctx = create_context(
            Profile::Debug,
            vec!["webgpu".to_string(), "audio".to_string()],
            Platform::Desktop { target: None },
        );
        let args = ctx.cargo_args();
        assert!(args.contains(&"--features".to_string()));
        assert!(args.contains(&"webgpu".to_string()));
        assert!(args.contains(&"audio".to_string()));
    }

    #[test]
    fn test_has_feature() {
        let ctx = create_context(
            Profile::Debug,
            vec!["webgpu".to_string()],
            Platform::Desktop { target: None },
        );
        assert!(ctx.has_feature("webgpu"));
        assert!(!ctx.has_feature("vulkan"));
    }

    #[test]
    fn test_has_any_feature() {
        let ctx = create_context(
            Profile::Debug,
            vec!["audio".to_string()],
            Platform::Desktop { target: None },
        );
        assert!(ctx.has_any_feature(&["audio", "video"]));
        assert!(!ctx.has_any_feature(&["video", "image"]));
    }

    #[test]
    fn test_has_all_features() {
        let ctx = create_context(
            Profile::Debug,
            vec!["audio".to_string(), "video".to_string()],
            Platform::Desktop { target: None },
        );
        assert!(ctx.has_all_features(&["audio", "video"]));
        assert!(!ctx.has_all_features(&["audio", "video", "image"]));
    }

    #[test]
    fn test_feature_count() {
        let ctx = create_context(
            Profile::Debug,
            vec!["feat1".to_string(), "feat2".to_string()],
            Platform::Desktop { target: None },
        );
        assert_eq!(ctx.feature_count(), 2);
    }

    #[test]
    fn test_platform_checks() {
        let android_ctx = create_context(
            Profile::Debug,
            vec![],
            Platform::Android {
                targets: vec!["aarch64-linux-android".to_string()],
            },
        );
        assert!(android_ctx.is_android());
        assert!(!android_ctx.is_web());
        assert!(!android_ctx.is_desktop());

        let web_ctx = create_context(
            Profile::Debug,
            vec![],
            Platform::Web {
                target: "web".to_string(),
            },
        );
        assert!(web_ctx.is_web());
        assert!(!web_ctx.is_android());
        assert!(!web_ctx.is_desktop());

        let desktop_ctx = create_context(Profile::Debug, vec![], Platform::Desktop { target: None });
        assert!(desktop_ctx.is_desktop());
        assert!(!desktop_ctx.is_android());
        assert!(!desktop_ctx.is_web());
    }
}
