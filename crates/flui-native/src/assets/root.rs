//! Where a desktop or Apple package keeps its assets (ADR-0384 §3–§4).

use std::ffi::OsString;
use std::path::PathBuf;

use super::PACKAGE_DIR;

/// The directory asset keys are joined to: `development` when it names an
/// absolute directory (a `flui run` of the project), else `flui_assets/`
/// under `resources`, the package's own directory. A relative
/// `development` would resolve against the current directory, which is never
/// a root, so it is ignored.
pub(super) fn package_root(
    development: Option<OsString>,
    resources: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(development) = development.map(PathBuf::from)
        && development.is_absolute()
    {
        return Some(development);
    }
    resources.map(|resources| resources.join(PACKAGE_DIR))
}

/// The package's own directory: the main bundle's resources on macOS and
/// iOS (`Contents/Resources` in a `.app`, the bundle's root on iOS, the
/// executable's directory outside a bundle), the executable's directory
/// elsewhere.
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub(super) fn package_resources() -> Option<PathBuf> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use objc2_foundation::NSBundle;
        NSBundle::mainBundle()
            .resourcePath()
            .map(|path| PathBuf::from(path.to_string()))
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::package_root;

    /// The development root wins when it is absolute; a relative one and none
    /// leave the package's `flui_assets/`, and no package leaves no root, never
    /// the current directory.
    ///
    /// Red-check: accept a relative development root, or fall back to `.`.
    #[test]
    fn a_package_root_is_never_the_current_directory() {
        let project = std::env::temp_dir().join("project");
        let package = std::env::temp_dir().join("package");
        assert_eq!(
            package_root(Some(project.clone().into()), Some(package.clone())),
            Some(project)
        );
        assert_eq!(
            package_root(Some("project".into()), Some(package.clone())),
            Some(package.join("flui_assets"))
        );
        assert_eq!(
            package_root(None, Some(package.clone())),
            Some(package.join("flui_assets"))
        );
        assert_eq!(package_root(Some("project".into()), None), None);
        assert_eq!(package_root(None, None::<PathBuf>), None);
    }
}
