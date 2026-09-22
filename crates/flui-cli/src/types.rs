//! Validated newtypes for the values `flui create` takes: a project name, an
//! organization identifier and the project's destination path. Each one can
//! only be constructed through its validation, so a command that holds one
//! never re-checks it.

use crate::error::{CliError, CliResult};
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::fmt::{self, Display, Formatter};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Reserved Rust keywords that cannot be used as project names.
///
/// **Sorted lexicographically** for use with `binary_search`.
const RESERVED_KEYWORDS: &[&str] = &[
    "Self", "abstract", "as", "async", "await", "become", "box", "break", "const", "continue",
    "crate", "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "type", "typeof", "unsafe",
    "unsized", "use", "virtual", "where", "while", "yield",
];

// ============================================================================
// ProjectName
// ============================================================================

/// A validated project name.
///
/// Project names must:
/// - Not be empty
/// - Contain only alphanumeric characters, hyphens, and underscores
/// - Not start with a number
/// - Not be a Rust keyword
///
/// # Implements
///
/// - `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`
/// - `Display`
/// - `AsRef<str>`, `Borrow<str>`
/// - `FromStr`, `TryFrom<String>`, `TryFrom<&str>`
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct ProjectName(String);

impl ProjectName {
    /// Create a new validated project name.
    ///
    /// # Errors
    ///
    /// Returns `CliError::InvalidProjectName` if the name:
    /// - Is empty
    /// - Contains characters other than alphanumeric, hyphens, or underscores
    /// - Starts with a number
    /// - Is a reserved Rust keyword
    pub(crate) fn new(name: impl Into<String>) -> CliResult<Self> {
        let name = name.into();
        Self::validate(&name)?;
        Ok(Self(name))
    }

    /// Get the project name as a string slice.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to a crate name (hyphens replaced with underscores).
    ///
    /// Rust crate names use underscores, not hyphens.
    #[must_use]
    pub(crate) fn to_crate_name(&self) -> String {
        self.0.replace('-', "_")
    }

    /// Validate a project name.
    fn validate(name: &str) -> CliResult<()> {
        if name.is_empty() {
            return Err(CliError::InvalidProjectName {
                name: name.to_string(),
                reason: "Project name cannot be empty".to_string(),
            });
        }

        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CliError::InvalidProjectName {
                name: name.to_string(),
                reason: "Project name must contain only alphanumeric characters, hyphens, and underscores".to_string(),
            });
        }

        if name.starts_with(|c: char| c.is_numeric()) {
            return Err(CliError::InvalidProjectName {
                name: name.to_string(),
                reason: "Project name cannot start with a number".to_string(),
            });
        }

        if RESERVED_KEYWORDS.binary_search(&name).is_ok() {
            return Err(CliError::InvalidProjectName {
                name: name.to_string(),
                reason: format!("'{name}' is a reserved Rust keyword"),
            });
        }

        Ok(())
    }
}

impl Display for ProjectName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl AsRef<str> for ProjectName {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for ProjectName {
    #[inline]
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Deref for ProjectName {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProjectName {
    type Err = CliError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for ProjectName {
    type Error = CliError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for ProjectName {
    type Error = CliError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProjectName> for String {
    fn from(name: ProjectName) -> Self {
        name.0
    }
}

// ============================================================================
// OrganizationId
// ============================================================================

/// A validated organization identifier in reverse domain notation.
///
/// Organization IDs should be in reverse domain notation (e.g., "com.example").
/// They are used to generate unique application identifiers for mobile platforms.
///
/// # Implements
///
/// - `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`
/// - `Default` (defaults to "com.example")
/// - `Display`
/// - `AsRef<str>`, `Borrow<str>`
/// - `FromStr`, `TryFrom<String>`, `TryFrom<&str>`
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct OrganizationId(String);

impl OrganizationId {
    /// Create a new organization identifier.
    ///
    /// The identifier should be in reverse domain notation (e.g., "com.example").
    ///
    /// # Errors
    ///
    /// Returns an error if the identifier:
    /// - Is empty
    /// - Has empty segments (e.g., "com..example")
    /// - Contains non-alphanumeric characters (except underscores)
    pub(crate) fn new(org: impl Into<String>) -> CliResult<Self> {
        let org = org.into();
        Self::validate(&org)?;
        Ok(Self(org))
    }

    /// Get the organization ID as a string slice.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the application ID by combining with a project name.
    ///
    /// The resulting ID is suitable for use as:
    /// - Android package name
    /// - iOS bundle identifier
    #[must_use]
    pub(crate) fn app_id(&self, name: &ProjectName) -> String {
        format!("{}.{}", self.0, name.to_crate_name())
    }

    fn validate(org: &str) -> CliResult<()> {
        if org.is_empty() {
            return Err(CliError::InvalidOrganizationId {
                id: org.to_string(),
                reason: "Organization ID cannot be empty".to_string(),
            });
        }

        for part in org.split('.') {
            if part.is_empty() {
                return Err(CliError::InvalidOrganizationId {
                    id: org.to_string(),
                    reason: "Organization ID has empty segment".to_string(),
                });
            }

            if !part.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(CliError::InvalidOrganizationId {
                    id: org.to_string(),
                    reason: "Organization ID segments must be alphanumeric".to_string(),
                });
            }
        }

        Ok(())
    }
}

impl Default for OrganizationId {
    /// Returns the default organization ID: "com.example"
    fn default() -> Self {
        Self("com.example".to_string())
    }
}

impl Display for OrganizationId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl AsRef<str> for OrganizationId {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for OrganizationId {
    #[inline]
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Deref for OrganizationId {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        &self.0
    }
}

impl FromStr for OrganizationId {
    type Err = CliError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for OrganizationId {
    type Error = CliError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for OrganizationId {
    type Error = CliError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<OrganizationId> for String {
    fn from(org: OrganizationId) -> Self {
        org.0
    }
}

// ============================================================================
// ProjectPath
// ============================================================================

/// A validated project path.
///
/// Wraps a `PathBuf` with validation to ensure the path is suitable for
/// project creation (i.e., the directory doesn't already exist).
///
/// # Implements
///
/// - `Debug`, `Clone`, `PartialEq`, `Eq`
/// - `Display` - shows the path
/// - `AsRef<Path>`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ProjectPath(PathBuf);

impl ProjectPath {
    /// Create a new project path from a name, optionally within a base directory.
    ///
    /// # Arguments
    ///
    /// * `name` - The validated project name
    /// * `base` - Optional base directory (defaults to current directory)
    ///
    /// # Errors
    ///
    /// Returns `CliError::DirectoryExists` if the resulting path already exists.
    pub(crate) fn new(name: &ProjectName, base: Option<PathBuf>) -> CliResult<Self> {
        let path = if let Some(base) = base {
            base.join(name.as_str())
        } else {
            PathBuf::from(name.as_str())
        };

        if path.exists() {
            return Err(CliError::DirectoryExists { path });
        }

        Ok(Self(path))
    }

    /// Get a reference to the underlying path.
    #[inline]
    #[must_use]
    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for ProjectPath {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Deref for ProjectPath {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Display for ProjectPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl From<ProjectPath> for PathBuf {
    fn from(path: ProjectPath) -> Self {
        path.0
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    mod project_name {
        use super::*;

        #[test]
        fn valid_names() {
            assert!(ProjectName::new("my-app").is_ok());
            assert!(ProjectName::new("my_app").is_ok());
            assert!(ProjectName::new("MyApp").is_ok());
            assert!(ProjectName::new("app123").is_ok());
            assert!(ProjectName::new("a").is_ok());
        }

        #[test]
        fn invalid_names() {
            assert!(ProjectName::new("").is_err());
            assert!(ProjectName::new("123app").is_err());
            assert!(ProjectName::new("my app").is_err());
            assert!(ProjectName::new("my.app").is_err());
            assert!(ProjectName::new("fn").is_err());
            assert!(ProjectName::new("struct").is_err());
        }

        #[test]
        fn to_crate_name() {
            let name = ProjectName::new("my-app").unwrap();
            assert_eq!(name.to_crate_name(), "my_app");

            let name = ProjectName::new("my_app").unwrap();
            assert_eq!(name.to_crate_name(), "my_app");
        }

        #[test]
        fn conversions() {
            // FromStr
            let name: ProjectName = "my-app".parse().unwrap();
            assert_eq!(name.as_str(), "my-app");

            // TryFrom<String>
            let name = ProjectName::try_from("my-app".to_string()).unwrap();
            assert_eq!(name.as_str(), "my-app");

            // TryFrom<&str>
            let name = ProjectName::try_from("my-app").unwrap();
            assert_eq!(name.as_str(), "my-app");
        }

        #[test]
        fn ordering() {
            let a = ProjectName::new("aaa").unwrap();
            let b = ProjectName::new("bbb").unwrap();
            assert!(a < b);
        }
    }

    mod organization_id {
        use super::*;

        #[test]
        fn valid_ids() {
            assert!(OrganizationId::new("com.example").is_ok());
            assert!(OrganizationId::new("org.rust_lang").is_ok());
            assert!(OrganizationId::new("io.github.user").is_ok());
            assert!(OrganizationId::new("com").is_ok());
        }

        #[test]
        fn invalid_ids() {
            assert!(OrganizationId::new("").is_err());
            assert!(OrganizationId::new("com..example").is_err());
            assert!(OrganizationId::new(".com.example").is_err());
            assert!(OrganizationId::new("com.example.").is_err());
            assert!(OrganizationId::new("com.exam ple").is_err());
        }

        #[test]
        fn default() {
            let org = OrganizationId::default();
            assert_eq!(org.as_str(), "com.example");
        }

        #[test]
        fn app_id_generation() {
            let org = OrganizationId::new("com.example").unwrap();
            let name = ProjectName::new("my-app").unwrap();
            assert_eq!(org.app_id(&name), "com.example.my_app");
        }
    }
}
