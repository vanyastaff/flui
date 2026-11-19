//! The application's assets in each package (ADR-0384 §3).
//!
//! Every regular file under each directory `[assets] directories` lists, but
//! a hidden one (a name starting with `.`, in any segment), is
//! copied to `flui_assets/<key>` inside the package, where `key` is its path
//! from the project root with `/` between segments: the key the application
//! reads it back by, through the host's asset bundle. A key is admitted by
//! the runtime's allow-list (`flui_platform::assets::AssetKey::try_new`),
//! which [`key_of`] states again here so the CLI links none of the framework;
//! `asset_keys_agree_with_the_runtime` holds the two to one rule.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::build::error::{BuildError, BuildResult};
use crate::config::AssetsConfig;

/// The directory inside a package that holds its assets; the runtime reads
/// it under the same name (`flui_native::assets::PACKAGE_DIR`).
pub(crate) const PACKAGE_DIR: &str = "flui_assets";

/// The most bytes a key may have, as `flui_platform::assets::MAX_ASSET_KEY_LEN`.
const MAX_KEY_LEN: usize = 1024;

/// The configuration field every refusal names.
const FIELD: &str = "[assets] directories";

/// One file a package carries: its key and where it is in the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackagedAsset {
    pub(crate) key: String,
    pub(crate) source: PathBuf,
}

/// The assets of the project in `project`, as its `config` declares them,
/// ordered by key.
///
/// # Errors
///
/// A listed directory that is not a directory of the project, a symbolic
/// link or anything else that is neither a directory nor a regular file
/// under one, a path that is not UTF-8 or whose key the runtime would
/// refuse, and two files whose keys differ only in case (one file on a
/// case-insensitive file system, as Windows' and macOS' are).
pub(crate) fn declared(project: &Path, config: &AssetsConfig) -> BuildResult<Vec<PackagedAsset>> {
    let mut assets = BTreeMap::<String, PathBuf>::new();
    for directory in &config.directories {
        let directory = directory.trim_end_matches('/');
        let root = key_of(directory)
            .map(|()| project.join(directory))
            .map_err(|reason| refuse(format!("{directory:?} {reason}")))?;
        let metadata = std::fs::symlink_metadata(&root).map_err(|error| {
            refuse(format!(
                "{directory:?} cannot be read under the project: {error}"
            ))
        })?;
        if !metadata.is_dir() {
            return Err(refuse(format!("{directory:?} is not a directory")));
        }
        // `symlink_metadata` sees only the last segment: a linked parent
        // would bring files from outside the project under its keys.
        let inside = |path: &Path| {
            std::fs::canonicalize(path).map_err(|error| {
                refuse(format!(
                    "{directory:?} cannot be resolved under the project: {error}"
                ))
            })
        };
        if !inside(&root)?.starts_with(inside(project)?) {
            return Err(refuse(format!(
                "{directory:?} leaves the project through a symbolic link; a package carries \
                 the project's own files only"
            )));
        }
        walk(&root, directory, &mut assets)?;
    }
    let mut folded = BTreeMap::<String, &str>::new();
    for key in assets.keys() {
        if let Some(other) = folded.insert(key.to_lowercase(), key) {
            return Err(refuse(format!(
                "{other:?} and {key:?} differ only in case, and are one file where the file \
                 system ignores case"
            )));
        }
    }
    Ok(assets
        .into_iter()
        .map(|(key, source)| PackagedAsset { key, source })
        .collect())
}

/// The assets of the project at or above `root`; none when there is no
/// `flui.toml` or it declares none.
///
/// # Errors
///
/// As [`declared`], and when `flui.toml` cannot be read.
pub(crate) fn of_project(root: &Path) -> BuildResult<Vec<PackagedAsset>> {
    let project = crate::config::nearest_project(root)
        .map_err(|error| BuildError::invalid_config("flui.toml", error.to_string()))?;
    match project {
        Some(project) => declared(&project.dir, &project.config.assets),
        None => Ok(Vec::new()),
    }
}

/// Copy `assets` to `flui_assets/` under `package_root`, replacing what an
/// earlier build left there, so an asset removed from the project leaves
/// the package too. With no assets, no directory is made.
///
/// # Errors
///
/// When a file cannot be copied.
pub(crate) fn stage(assets: &[PackagedAsset], package_root: &Path) -> BuildResult<()> {
    let destination = package_root.join(PACKAGE_DIR);
    match std::fs::remove_dir_all(&destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    for asset in assets {
        let mut target = destination.clone();
        target.extend(asset.key.split('/'));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                BuildError::Other(format!("could not create {}: {error}", parent.display()))
            })?;
        }
        std::fs::copy(&asset.source, &target).map_err(|error| {
            BuildError::Other(format!(
                "could not copy {} to {}: {error}",
                asset.source.display(),
                target.display()
            ))
        })?;
    }
    if !assets.is_empty() {
        crate::ui::debug(format!(
            "{} asset(s) staged in {}",
            assets.len(),
            destination.display()
        ));
    }
    Ok(())
}

/// Adds every regular file under `dir`, whose key is `prefix`, to `assets`.
fn walk(dir: &Path, prefix: &str, assets: &mut BTreeMap<String, PathBuf>) -> BuildResult<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| refuse(format!("{} is not a UTF-8 name", path.display())))?;
        // A hidden file or directory (`.gitkeep`, `.DS_Store`, `.git/`) is
        // not an asset: Android's packagers drop it from an APK by default,
        // so no package carries it (ADR-0384 §3).
        if name.starts_with('.') {
            continue;
        }
        let key = format!("{prefix}/{name}");
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(refuse(format!(
                "{} is a symbolic link; a package carries files only",
                path.display()
            )));
        }
        if kind.is_dir() {
            walk(&path, &key, assets)?;
        } else if kind.is_file() {
            key_of(&key).map_err(|reason| refuse(format!("{} {reason}", path.display())))?;
            // A directory listed twice, or inside another listed one, yields
            // the same file under the same key.
            assets.insert(key, path);
        } else {
            return Err(refuse(format!(
                "{} is neither a file nor a directory",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Whether the runtime admits `key` (`AssetKey::try_new`), or why not.
fn key_of(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("is empty");
    }
    if key.len() > MAX_KEY_LEN {
        return Err("is longer than an asset key may be (1024 bytes)");
    }
    if key.chars().any(|character| {
        matches!(character, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            || character.is_control()
    }) {
        return Err(
            "holds a character no Windows file name may (`\\ : * ? \" < > |`) or a control \
             character, which no asset key may",
        );
    }
    for segment in key.split('/') {
        match segment {
            "" => return Err("has an empty segment (a leading `/` or `//`)"),
            "." | ".." => return Err("has a `.` or `..` segment"),
            _ if segment.ends_with(['.', ' ']) || is_device_name(segment) => {
                return Err(
                    "has a segment Windows reserves or trims (a trailing `.` or space, or a \
                     device name such as `NUL` or `COM1`)",
                );
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether Windows takes `segment` for a device rather than a file: its
/// stem before the first `.`, trailing spaces trimmed, is a reserved name in
/// any case (`CON`, `CONIN$`, `CONOUT$`, `PRN`, `AUX`, `NUL`, `COM` or `LPT`
/// and a digit, superscript `¹²³` included).
fn is_device_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "CONIN$" | "CONOUT$" | "PRN" | "AUX" | "NUL"
    ) {
        return true;
    }
    let Some(tail) = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
    else {
        return false;
    };
    let mut chars = tail.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '¹' | '²' | '³'), None)
    )
}

fn refuse(reason: String) -> BuildError {
    BuildError::invalid_config(FIELD, reason)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{PackagedAsset, declared, key_of, stage};
    use crate::config::AssetsConfig;

    /// The directory the runtime reads, written out: `flui_native::assets`
    /// reads this name, so renaming the CLI's constant alone must fail here.
    const PACKAGE_DIR: &str = "flui_assets";

    fn config(directories: &[&str]) -> AssetsConfig {
        AssetsConfig {
            directories: directories.iter().map(|&dir| dir.to_owned()).collect(),
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("a file has a directory"))
            .expect("the fixture directory is writable");
        std::fs::write(path, bytes).expect("the fixture is writable");
    }

    /// The CLI admits exactly the keys the runtime does, so no package
    /// carries a file its application cannot name, and no name the
    /// application uses is refused at build.
    ///
    /// Red-check: drop the `:` from `key_of`'s forbidden characters.
    #[test]
    fn asset_keys_agree_with_the_runtime() {
        let long = "a".repeat(1025);
        let keys = [
            "assets/logo.png",
            "assets/фото 1.png",
            "assets/.hidden",
            "assets/a..b",
            "",
            "/assets/logo.png",
            "assets//logo.png",
            "assets/./logo.png",
            "assets/../logo.png",
            "assets\\logo.png",
            "C:/logo.png",
            "assets/logo\n.png",
            "assets/logo\u{7f}.png",
            "assets/logo?.png",
            "assets/a|b.png",
            "assets/NUL",
            "assets/com1.txt",
            "assets/console.png",
            "assets/CONIN$",
            "assets/nul .txt",
            "assets/COM¹.png",
            "assets/logo.png.",
            "assets /logo.png",
            &long,
            &long[1..],
        ];
        for key in keys {
            assert_eq!(
                key_of(key).is_ok(),
                flui_platform::assets::AssetKey::try_new(key).is_ok(),
                "{key:?}"
            );
        }
        assert_eq!(super::MAX_KEY_LEN, flui_platform::assets::MAX_ASSET_KEY_LEN);
    }

    /// Every regular file under each listed directory is an asset keyed by
    /// its path from the project root, at any depth and in key order; a
    /// directory listed twice or inside another yields each file once, and
    /// staging replaces what an earlier build staged; a hidden file or
    /// directory is no asset.
    ///
    /// Red-check: key a file by its path from the listed directory, or stage
    /// without removing the earlier `flui_assets/`.
    #[test]
    fn a_build_stages_every_declared_file_under_flui_assets() {
        let project = tempfile::tempdir().expect("fixture");
        write(&project.path().join("assets/logo.png"), b"logo");
        write(
            &project.path().join("assets/images/icons/home.svg"),
            b"home",
        );
        write(&project.path().join("data/levels.json"), b"[]");
        write(&project.path().join("src/main.rs"), b"fn main() {}");
        write(&project.path().join("assets/.gitkeep"), b"");
        write(&project.path().join("assets/.cache/thumb.png"), b"hidden");

        let assets = declared(
            project.path(),
            &config(&["assets", "data/", "assets/images"]),
        )
        .expect("the declared directories are the project's");
        let keys: Vec<&str> = assets.iter().map(|asset| asset.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "assets/images/icons/home.svg",
                "assets/logo.png",
                "data/levels.json"
            ]
        );

        let package = tempfile::tempdir().expect("fixture");
        write(
            &package.path().join(PACKAGE_DIR).join("assets/removed.png"),
            b"old",
        );
        stage(&assets, package.path()).expect("the package is writable");
        let staged = package.path().join(PACKAGE_DIR);
        assert_eq!(
            std::fs::read(staged.join("assets/images/icons/home.svg")).expect("staged"),
            b"home"
        );
        assert_eq!(
            std::fs::read(staged.join("data/levels.json")).expect("staged"),
            b"[]"
        );
        assert!(
            !staged.join("assets/removed.png").exists(),
            "an earlier build's asset is gone"
        );
        assert!(!staged.join("src").exists());

        let empty = tempfile::tempdir().expect("fixture");
        stage(&[], empty.path()).expect("nothing to stage");
        assert!(!empty.path().join(PACKAGE_DIR).exists());
    }

    /// A build refuses what no package can carry or no key can name: a
    /// directory outside the project, a missing one, a symbolic link (a file,
    /// the listed directory or a parent of it), and
    /// two files one case-insensitive file system holds as one.
    ///
    /// Red-check: follow symbolic links, or compare keys case-sensitively.
    #[test]
    fn a_build_refuses_what_no_package_can_carry() {
        let project = tempfile::tempdir().expect("fixture");
        write(&project.path().join("assets/logo.png"), b"logo");
        for outside in ["../assets", "/etc", "missing", "assets/logo.png"] {
            assert!(
                declared(project.path(), &config(&[outside])).is_err(),
                "{outside:?}"
            );
        }

        let cased = tempfile::tempdir().expect("fixture");
        write(&cased.path().join("assets/Logo.png"), b"a");
        write(&cased.path().join("assets/logo.png"), b"b");
        if cased.path().join("assets/Logo.png").exists()
            && std::fs::read(cased.path().join("assets/Logo.png")).expect("read") == b"a"
        {
            let refusal = declared(cased.path(), &config(&["assets"]))
                .expect_err("keys that differ only in case");
            assert!(
                refusal.to_string().contains("differ only in case"),
                "{refusal}"
            );
        }

        #[cfg(unix)]
        {
            let linked = tempfile::tempdir().expect("fixture");
            write(&linked.path().join("secret.txt"), b"secret");
            std::fs::create_dir_all(linked.path().join("assets")).expect("fixture");
            std::os::unix::fs::symlink(
                linked.path().join("secret.txt"),
                linked.path().join("assets/link.txt"),
            )
            .expect("fixture");
            let refusal =
                declared(linked.path(), &config(&["assets"])).expect_err("a symbolic link");
            assert!(refusal.to_string().contains("symbolic link"), "{refusal}");

            // A listed directory that is a link, and one under a linked
            // parent, both reach files outside the project.
            let outside = tempfile::tempdir().expect("fixture");
            write(&outside.path().join("assets/secret.txt"), b"secret");
            let project = tempfile::tempdir().expect("fixture");
            std::os::unix::fs::symlink(outside.path(), project.path().join("link"))
                .expect("fixture");
            std::os::unix::fs::symlink(
                outside.path().join("assets"),
                project.path().join("assets"),
            )
            .expect("fixture");
            for listed in ["assets", "link/assets"] {
                let refusal = declared(project.path(), &config(&[listed]))
                    .expect_err("a directory outside the project");
                assert!(
                    refusal.to_string().contains("symbolic link")
                        || refusal.to_string().contains("not a directory"),
                    "{listed:?}: {refusal}"
                );
            }
        }

        assert_eq!(
            declared(project.path(), &config(&[])).expect("no assets"),
            Vec::<PackagedAsset>::new()
        );
    }
}
