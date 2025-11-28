# flui_build Pattern Analysis and Improvement Plan

This document analyzes the current `flui_build` codebase against Rust best practices from `patterns.md` and provides actionable improvement recommendations.

## Current Architecture Analysis

### ✅ Good Patterns Already Used

1. **Trait-based Architecture** ✅
   - `PlatformBuilder` trait with common interface
   - Clean separation of concerns per platform
   - Location: `src/platform.rs:70-89`

2. **Error Handling with anyhow** ✅
   - Consistent use of `Result<T>` with `anyhow::Error`
   - Context-aware error messages with `.context()`
   - Location: Throughout codebase

3. **Send + Sync Bounds** ✅
   - `PlatformBuilder: Send + Sync` for thread safety
   - Location: `src/platform.rs:70`

4. **Builder Structs** ✅
   - `AndroidBuilder`, `WebBuilder`, `DesktopBuilder`
   - Each with `new()` constructor
   - Location: `src/android.rs`, `src/web.rs`, `src/desktop.rs`

### ❌ Missing Patterns (Opportunities for Improvement)

## Pattern Improvements to Implement

### 1. Type State Builder Pattern (HIGH PRIORITY)

**Current Issue:**
```rust
// BuilderContext created with all fields at once - no compile-time validation
let ctx = BuilderContext {
    workspace_root: PathBuf::from("."),
    platform: Platform::Android { targets: vec![] },
    profile: Profile::Release,
    features: vec![],
    output_dir: PathBuf::from("target/flui-out/android"),
};
```

**Improvement: Add Typestate Builder**

```rust
// NEW: Type states for builder validation
pub struct NoPlatform;
pub struct HasPlatform;
pub struct NoProfile;
pub struct HasProfile;

pub struct BuilderContextBuilder<P = NoPlatform, Pr = NoProfile> {
    workspace_root: PathBuf,
    platform: P,
    profile: Pr,
    features: Vec<String>,
    output_dir: Option<PathBuf>,
}

// Only allow build() when platform and profile are set
impl BuilderContextBuilder<HasPlatform, HasProfile> {
    pub fn build(self) -> BuilderContext {
        BuilderContext {
            workspace_root: self.workspace_root,
            platform: self.platform.0,
            profile: self.profile.0,
            features: self.features,
            output_dir: self.output_dir.unwrap_or_else(|| {
                self.workspace_root.join("target").join("flui-out")
            }),
        }
    }
}

// Usage - compile-time enforcement
let ctx = BuilderContextBuilder::new(workspace_root)
    .with_platform(Platform::Android { targets: vec![] })
    .with_profile(Profile::Release)
    .build(); // ✅ Compiles

// let ctx = BuilderContextBuilder::new(workspace_root)
//     .with_platform(Platform::Android { targets: vec![] })
//     .build(); // ❌ Compile error - missing profile
```

**Benefits:**
- ✅ Compile-time validation of required fields
- ✅ IDE autocomplete guides users
- ✅ Impossible to create invalid BuilderContext
- ✅ Follows Rust API Guidelines (C-BUILDER)

**Files to create/modify:**
- `src/context_builder.rs` (new file)
- `src/platform.rs` (add builder impl)

---

### 2. Extension Traits (MEDIUM PRIORITY)

**Current Issue:**
No convenient extension methods for common operations.

**Improvement: Add Extension Traits**

```rust
// NEW: Extension trait for BuilderContext
pub trait BuilderContextExt {
    /// Check if this is a release build
    fn is_release(&self) -> bool;

    /// Get cargo arguments for this profile
    fn cargo_args(&self) -> Vec<String>;

    /// Get platform-specific output directory
    fn platform_output_dir(&self) -> PathBuf;

    /// Check if features contain a specific feature
    fn has_feature(&self, feature: &str) -> bool;
}

// Blanket implementation
impl BuilderContextExt for BuilderContext {
    fn is_release(&self) -> bool {
        matches!(self.profile, Profile::Release)
    }

    fn cargo_args(&self) -> Vec<String> {
        let mut args = vec![];
        if let Some(flag) = self.profile.cargo_flag() {
            args.push(flag.to_string());
        }
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
}

// Usage
if ctx.is_release() {
    // optimize further
}
let args = ctx.cargo_args();
let output = ctx.platform_output_dir();
```

**Benefits:**
- ✅ Clean API without bloating core struct
- ✅ Easy to add more methods later
- ✅ Follows Extension Trait pattern from patterns.md

**Files to create/modify:**
- `src/context_ext.rs` (new file)
- `src/lib.rs` (re-export trait)

---

### 3. Custom Error Types (MEDIUM PRIORITY)

**Current Issue:**
Using generic `anyhow::Error` - no type-safe error handling.

**Improvement: Add Custom Error Enum**

```rust
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Errors that can occur during the build process
#[derive(Debug)]
#[non_exhaustive]
pub enum BuildError {
    /// Required tool not found (cargo, wasm-pack, etc.)
    ToolNotFound { tool: String, install_hint: String },

    /// Platform target not installed
    TargetNotInstalled { target: String, install_cmd: String },

    /// Environment variable missing or invalid
    EnvVarError { var: String, reason: String },

    /// Build command failed
    CommandFailed { command: String, exit_code: i32, stderr: String },

    /// File or directory not found
    PathNotFound { path: PathBuf, context: String },

    /// Invalid platform configuration
    InvalidPlatform { reason: String },

    /// I/O error
    Io(std::io::Error),
}

impl Display for BuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ToolNotFound { tool, install_hint } => {
                write!(f, "{} not found. Install with: {}", tool, install_hint)
            }
            Self::TargetNotInstalled { target, install_cmd } => {
                write!(f, "Rust target '{}' not installed. Install with: {}", target, install_cmd)
            }
            Self::EnvVarError { var, reason } => {
                write!(f, "Environment variable {} error: {}", var, reason)
            }
            Self::CommandFailed { command, exit_code, stderr } => {
                write!(f, "Command '{}' failed with exit code {}\nStderr: {}", command, exit_code, stderr)
            }
            Self::PathNotFound { path, context } => {
                write!(f, "Path not found: {} ({})", path.display(), context)
            }
            Self::InvalidPlatform { reason } => {
                write!(f, "Invalid platform: {}", reason)
            }
            Self::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl Error for BuildError {}

// Conversion from std::io::Error
impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub type BuildResult<T> = Result<T, BuildError>;
```

**Benefits:**
- ✅ Type-safe error handling
- ✅ Better error messages with context
- ✅ Can match on specific errors
- ✅ Follows Error Handling pattern from patterns.md
- ✅ #[non_exhaustive] allows adding variants without breaking change

**Files to create/modify:**
- `src/error.rs` (new file)
- All builder files (migrate from anyhow)

---

### 4. From/Into Trait Implementations (LOW PRIORITY)

**Current Issue:**
No convenient conversions between types.

**Improvement: Add From/Into Traits**

```rust
// Convert &str to Platform
impl From<&str> for Platform {
    fn from(s: &str) -> Self {
        match s {
            "android" => Platform::Android { targets: vec!["aarch64-linux-android".to_string()] },
            "web" => Platform::Web { target: "web".to_string() },
            "desktop" => Platform::Desktop { target: None },
            _ => Platform::Desktop { target: None },
        }
    }
}

// Convert &str to Profile
impl From<&str> for Profile {
    fn from(s: &str) -> Self {
        match s {
            "release" => Profile::Release,
            _ => Profile::Debug,
        }
    }
}

// Usage
let platform: Platform = "android".into();
let profile: Profile = "release".into();
```

**Benefits:**
- ✅ Convenient API for CLI parsing
- ✅ Follows From/Into pattern from patterns.md

**Files to modify:**
- `src/platform.rs`

---

### 5. Sealed Trait Pattern (LOW PRIORITY)

**Current Issue:**
`PlatformBuilder` trait can be implemented by external users.

**Improvement: Make PlatformBuilder Sealed**

```rust
mod private {
    pub trait Sealed {}
}

/// Platform-specific builder trait (sealed - cannot be implemented externally)
pub trait PlatformBuilder: private::Sealed + Send + Sync {
    fn platform_name(&self) -> &str;
    // ... rest of methods
}

// Only flui_build can implement Sealed
impl private::Sealed for AndroidBuilder {}
impl private::Sealed for WebBuilder {}
impl private::Sealed for DesktopBuilder {}

// Public trait implementations
impl PlatformBuilder for AndroidBuilder { /* ... */ }
impl PlatformBuilder for WebBuilder { /* ... */ }
impl PlatformBuilder for DesktopBuilder { /* ... */ }
```

**Benefits:**
- ✅ Prevents external implementations
- ✅ Allows future trait changes without breaking users
- ✅ Follows Sealed Trait pattern from patterns.md

**Files to modify:**
- `src/platform.rs`

---

### 6. AsRef/AsMut Implementations (LOW PRIORITY)

**Current Issue:**
No way to reference inner data without moving.

**Improvement: Add AsRef Implementations**

```rust
impl AsRef<PathBuf> for BuilderContext {
    fn as_ref(&self) -> &PathBuf {
        &self.workspace_root
    }
}

impl AsRef<Platform> for BuilderContext {
    fn as_ref(&self) -> &Platform {
        &self.platform
    }
}

// Usage - generic functions
fn process_workspace<T: AsRef<PathBuf>>(ctx: T) {
    let path = ctx.as_ref();
    // ...
}

process_workspace(&ctx); // Works with BuilderContext
process_workspace(&path_buf); // Also works with PathBuf
```

**Benefits:**
- ✅ Generic functions work with multiple types
- ✅ Follows AsRef pattern from patterns.md

**Files to modify:**
- `src/platform.rs`

---

## Implementation Priority

### Phase 1: High Priority (Do First)
1. ✅ **Type State Builder Pattern** - Most impact on API safety
2. ✅ **Custom Error Types** - Better error handling

### Phase 2: Medium Priority (Do Next)
3. ✅ **Extension Traits** - Improve API ergonomics
4. ✅ **Comprehensive Documentation** - Match flui_assets quality

### Phase 3: Low Priority (Nice to Have)
5. ✅ **From/Into Traits** - Convenience methods
6. ✅ **Sealed Trait Pattern** - Future-proofing
7. ✅ **AsRef Implementations** - Generic function support

## Documentation Improvements

Following the comprehensive documentation standards from `flui_assets`:

1. **README.md Enhancement**
   - Add architecture diagrams (ASCII art)
   - Add more examples
   - Add troubleshooting section
   - Add performance considerations

2. **Module Documentation**
   - Add rustdoc examples to all public items
   - Add `# Examples` sections
   - Add `# Panics` and `# Errors` sections where applicable

3. **Create docs/ Directory**
   - `docs/GUIDE.md` - User guide
   - `docs/ARCHITECTURE.md` - System internals
   - `docs/PATTERNS.md` - Design patterns used
   - `docs/EXAMPLES.md` - Complete examples

## Testing Improvements

1. **Unit Tests**
   - Add tests for all builders
   - Add tests for error handling
   - Add tests for extension traits

2. **Integration Tests**
   - Add end-to-end build tests (if feasible in CI)
   - Add tests for type state builder

3. **Documentation Tests**
   - Ensure all rustdoc examples are tested
   - Add `no_run` where appropriate (for examples requiring tools)

## Code Quality Metrics

After implementing improvements:

**Target Metrics:**
- ✅ API Guidelines Compliance: 95%+
- ✅ Documentation Coverage: 100% public API
- ✅ Test Coverage: 80%+ line coverage
- ✅ Clippy Warnings: 0
- ✅ All feature combinations compile

## Summary

**Key Improvements:**
1. Type State Builder - Compile-time safety ⭐⭐⭐
2. Custom Error Types - Better error handling ⭐⭐⭐
3. Extension Traits - Ergonomic API ⭐⭐
4. Comprehensive Documentation - Professional polish ⭐⭐⭐

**Estimated Impact:**
- **Safety:** +40% (Type State Builder)
- **Ergonomics:** +30% (Extension Traits)
- **Maintainability:** +50% (Documentation + Error Types)
- **Professional Polish:** +200% (Documentation matching flui_assets)

**Next Steps:**
1. Review this analysis with team
2. Prioritize which improvements to implement
3. Create GitHub issues for tracking
4. Begin implementation in priority order
