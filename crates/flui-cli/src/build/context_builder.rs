/// Type state builder for `BuilderContext` with compile-time validation.
///
/// This module implements the Type State pattern to ensure `BuilderContext`
/// is always constructed with all required fields. The type system prevents
/// invalid configurations at compile time.
///
/// # Type States
///
/// - `NoPlatform` / `HasPlatform` - Platform configuration
/// - `NoProfile` / `HasProfile` - Build profile (debug/release)
///
/// # Example
///
/// ```rust
/// use crate::build::*;
/// use std::path::PathBuf;
///
/// // ✅ This compiles - all required fields set
/// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
///     .with_platform(Platform::Android { targets: vec!["aarch64-linux-android".to_string()] })
///     .with_profile(Profile::Release)
///     .build();
///
/// // ❌ This doesn't compile - missing profile
/// // let ctx = BuilderContextBuilder::new(PathBuf::from("."))
/// //     .with_platform(Platform::Android { targets: vec![] })
/// //     .build();
/// ```
use std::path::PathBuf;

use crate::build::platform::{AppBundle, BuildUnit, BuilderContext, Platform, Profile};

/// Type state: No platform set
#[derive(Debug)]
pub struct NoPlatform;

/// Type state: Platform is set
#[derive(Debug)]
pub struct HasPlatform(pub(crate) Platform);

/// Type state: No profile set
#[derive(Debug)]
pub struct NoProfile;

/// Type state: Profile is set
#[derive(Debug)]
pub struct HasProfile(pub(crate) Profile);

/// Builder for `BuilderContext` with compile-time validation.
///
/// Uses the Type State pattern to ensure all required fields are set before `build()`.
///
/// # Type Parameters
///
/// - `P`: Platform state (`NoPlatform` or `HasPlatform`)
/// - `Pr`: Profile state (`NoProfile` or `HasProfile`)
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use crate::build::*;
/// use std::path::PathBuf;
///
/// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
///     .with_platform(Platform::Android { targets: vec!["aarch64-linux-android".to_string()] })
///     .with_profile(Profile::Release)
///     .build();
/// ```
///
/// ## With Optional Features
///
/// ```rust
/// use crate::build::*;
/// use std::path::PathBuf;
///
/// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
///     .with_platform(Platform::Web { target: "web".to_string() })
///     .with_profile(Profile::Debug)
///     .with_output_dir(PathBuf::from("custom/output"))
///     .build();
/// ```
///
/// ## Type Safety
///
/// ```compile_fail
/// use crate::build::*;
/// use std::path::PathBuf;
///
/// // This will not compile - missing profile
/// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
///     .with_platform(Platform::Android { targets: vec![] })
///     .build();
/// ```
#[derive(Debug)]
pub struct BuilderContextBuilder<P = NoPlatform, Pr = NoProfile> {
    workspace_root: PathBuf,
    platform: P,
    target: BuildUnit,
    profile: Pr,
    output_dir: Option<PathBuf>,
    bundle: Option<AppBundle>,
}

// Initial builder creation
impl BuilderContextBuilder<NoPlatform, NoProfile> {
    /// Create a new builder with the workspace root.
    ///
    /// # Arguments
    ///
    /// * `workspace_root` - Root directory of the workspace
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::build::*;
    /// use std::path::PathBuf;
    ///
    /// let builder = BuilderContextBuilder::new(PathBuf::from("."));
    /// ```
    #[must_use]
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            platform: NoPlatform,
            target: BuildUnit::DefaultBinary,
            profile: NoProfile,
            output_dir: None,
            bundle: None,
        }
    }
}

// Platform configuration - works regardless of profile state
impl<Pr> BuilderContextBuilder<NoPlatform, Pr> {
    /// Set the target platform.
    ///
    /// # Arguments
    ///
    /// * `platform` - Target platform (Android, Web, or Desktop)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::build::*;
    /// use std::path::PathBuf;
    ///
    /// let builder = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Android {
    ///         targets: vec!["aarch64-linux-android".to_string()],
    ///     });
    /// ```
    pub fn with_platform(self, platform: Platform) -> BuilderContextBuilder<HasPlatform, Pr> {
        BuilderContextBuilder {
            workspace_root: self.workspace_root,
            platform: HasPlatform(platform),
            target: self.target,
            profile: self.profile,
            output_dir: self.output_dir,
            bundle: self.bundle,
        }
    }
}

// Profile configuration - works regardless of platform state
impl<P> BuilderContextBuilder<P, NoProfile> {
    /// Set the build profile.
    ///
    /// # Arguments
    ///
    /// * `profile` - Build profile (Debug or Release)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::build::*;
    /// use std::path::PathBuf;
    ///
    /// let builder = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_profile(Profile::Release);
    /// ```
    pub fn with_profile(self, profile: Profile) -> BuilderContextBuilder<P, HasProfile> {
        BuilderContextBuilder {
            workspace_root: self.workspace_root,
            platform: self.platform,
            target: self.target,
            profile: HasProfile(profile),
            output_dir: self.output_dir,
            bundle: self.bundle,
        }
    }
}

// Optional fields - available regardless of state
impl<P, Pr> BuilderContextBuilder<P, Pr> {
    /// Select which cargo package or example to compile.
    ///
    /// # Arguments
    ///
    /// * `target` - The [`BuildUnit`] to compile
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::build::*;
    /// use std::path::PathBuf;
    ///
    /// let builder = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_target(BuildUnit::Example("material_demo".to_string()));
    /// ```
    #[must_use]
    pub fn with_target(mut self, target: BuildUnit) -> Self {
        self.target = target;
        self
    }

    /// Stage the built executable into an application bundle with this identity.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::build::*;
    /// use std::path::PathBuf;
    ///
    /// let builder = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_bundle(AppBundle::new("My App", "com.example"));
    /// ```
    #[must_use]
    pub fn with_bundle(mut self, bundle: AppBundle) -> Self {
        self.bundle = Some(bundle);
        self
    }

    /// Set custom output directory.
    ///
    /// If not set, defaults to `workspace_root/target/flui-out/`.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - Custom output directory
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::build::*;
    /// use std::path::PathBuf;
    ///
    /// let builder = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_output_dir(PathBuf::from("custom/output"));
    /// ```
    #[must_use]
    pub fn with_output_dir(mut self, output_dir: PathBuf) -> Self {
        self.output_dir = Some(output_dir);
        self
    }
}

// Build only when both platform and profile are set
impl BuilderContextBuilder<HasPlatform, HasProfile> {
    /// Build the final `BuilderContext`.
    ///
    /// This method is only available when both platform and profile have been set,
    /// ensuring compile-time validation of required fields.
    ///
    /// # Returns
    ///
    /// A fully configured `BuilderContext`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crate::build::*;
    /// use std::path::PathBuf;
    ///
    /// let ctx = BuilderContextBuilder::new(PathBuf::from("."))
    ///     .with_platform(Platform::Android {
    ///         targets: vec!["aarch64-linux-android".to_string()],
    ///     })
    ///     .with_profile(Profile::Release)
    ///     .build();
    ///
    /// assert_eq!(ctx.profile, Profile::Release);
    /// ```
    #[must_use]
    pub fn build(self) -> BuilderContext {
        let output_dir = self.output_dir.unwrap_or_else(|| {
            self.workspace_root
                .join("target")
                .join("flui-out")
                .join(self.platform.0.name())
        });

        BuilderContext {
            workspace_root: self.workspace_root,
            platform: self.platform.0,
            target: self.target,
            profile: self.profile.0,
            output_dir,
            bundle: self.bundle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() {
        let ctx = BuilderContextBuilder::new(PathBuf::from("."))
            .with_platform(Platform::Android {
                targets: vec!["aarch64-linux-android".to_string()],
            })
            .with_profile(Profile::Release)
            .build();

        assert_eq!(ctx.profile, Profile::Release);
    }

    #[test]
    fn test_builder_custom_output_dir() {
        let custom_dir = PathBuf::from("custom/output");
        let ctx = BuilderContextBuilder::new(PathBuf::from("."))
            .with_platform(Platform::Android {
                targets: vec!["aarch64-linux-android".to_string()],
            })
            .with_profile(Profile::Release)
            .with_output_dir(custom_dir.clone())
            .build();

        assert_eq!(ctx.output_dir, custom_dir);
    }

    #[test]
    fn test_builder_default_output_dir() {
        let workspace = PathBuf::from(".");
        let ctx = BuilderContextBuilder::new(workspace.clone())
            .with_platform(Platform::Android {
                targets: vec!["aarch64-linux-android".to_string()],
            })
            .with_profile(Profile::Release)
            .build();

        let expected = workspace.join("target").join("flui-out").join("android");
        assert_eq!(ctx.output_dir, expected);
    }

    #[test]
    fn test_builder_order_independence() {
        // Profile before platform
        let ctx1 = BuilderContextBuilder::new(PathBuf::from("."))
            .with_profile(Profile::Release)
            .with_platform(Platform::Android {
                targets: vec!["aarch64-linux-android".to_string()],
            })
            .build();

        // Platform before profile
        let ctx2 = BuilderContextBuilder::new(PathBuf::from("."))
            .with_platform(Platform::Android {
                targets: vec!["aarch64-linux-android".to_string()],
            })
            .with_profile(Profile::Release)
            .build();

        assert_eq!(ctx1.profile, ctx2.profile);
    }
}
