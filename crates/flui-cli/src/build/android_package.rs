//! An APK without Gradle: the SDK's own `build-tools` do everything a
//! `hasCode="false"` NativeActivity app needs.
//!
//! 1. `aapt2 link` turns `AndroidManifest.xml` into a base archive (no
//!    resources: the manifest carries no icon, the platform's default one
//!    applies).
//! 2. The native libraries go in as `lib/<abi>/lib<name>.so`, stored
//!    uncompressed: with `targetSdk >= 23` the platform maps them straight
//!    out of the archive instead of extracting, which needs page alignment.
//! 3. `zipalign -p 4` provides that alignment.
//! 4. `apksigner` signs with the debug keystore every Android SDK user has
//!    (`~/.android/debug.keystore`, created with `keytool` when absent).
//!
//! A project that grows a Java/Kotlin side keeps working: when
//! `platforms/android/gradlew` exists the builder uses Gradle instead and
//! this module is never called.

use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use tokio::process::Command;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::build::error::{BuildError, BuildResult};
use crate::build::util::process;

/// What the packager needs to know about the app.
pub(crate) struct ApkSpec<'a> {
    /// The scaffolded manifest (`platforms/android/app/src/main/AndroidManifest.xml`).
    pub(crate) manifest: &'a Path,
    /// Reverse-DNS application id; injected into the manifest for `aapt2`.
    pub(crate) package: &'a str,
    /// `(abi, path)` for every native library, e.g. `("arm64-v8a", …/libapp.so)`.
    pub(crate) native_libs: &'a [(String, PathBuf)],
    /// Scratch directory for the intermediate archives.
    pub(crate) work_dir: &'a Path,
    /// Where the signed APK is written.
    pub(crate) output: &'a Path,
}

/// The SDK pieces the packager runs, resolved once.
pub(crate) struct BuildTools {
    aapt2: PathBuf,
    zipalign: PathBuf,
    apksigner: PathBuf,
    android_jar: PathBuf,
    /// The API level of `android_jar`, used as the target SDK.
    api_level: u32,
    /// `JAVA_HOME` for `apksigner` and `keytool`, when known.
    java_home: Option<PathBuf>,
}

const MIN_SDK: u32 = 21;
const DEBUG_ALIAS: &str = "androiddebugkey";
const DEBUG_PASSWORD: &str = "android";

impl BuildTools {
    /// The highest installed `build-tools` and `platforms` of `sdk_root`.
    pub(crate) fn locate(sdk_root: &Path, java_home: Option<PathBuf>) -> BuildResult<Self> {
        let build_tools = newest_subdir(&sdk_root.join("build-tools"), |name| {
            name.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .ok_or_else(|| BuildError::ToolNotFound {
            tool: "Android build-tools (aapt2, zipalign, apksigner)".into(),
            install_hint: "sdkmanager \"build-tools;35.0.0\"".into(),
        })?;
        let (platform, api_level) =
            newest_platform(&sdk_root.join("platforms")).ok_or_else(|| {
                BuildError::ToolNotFound {
                    tool: "an Android platform (android.jar)".into(),
                    install_hint: "sdkmanager \"platforms;android-35\"".into(),
                }
            })?;
        let exe = |name: &str| {
            let candidates = if cfg!(windows) {
                vec![format!("{name}.bat"), format!("{name}.exe")]
            } else {
                vec![name.to_string()]
            };
            candidates
                .into_iter()
                .map(|file| build_tools.join(file))
                .find(|path| path.is_file())
                .ok_or_else(|| BuildError::ToolNotFound {
                    tool: format!("{name} in {}", build_tools.display()),
                    install_hint: "sdkmanager \"build-tools;35.0.0\"".into(),
                })
        };
        Ok(Self {
            aapt2: exe("aapt2")?,
            zipalign: exe("zipalign")?,
            apksigner: exe("apksigner")?,
            android_jar: platform.join("android.jar"),
            api_level,
            java_home,
        })
    }

    /// Build, align and sign `spec.output`.
    pub(crate) async fn package(&self, spec: &ApkSpec<'_>) -> BuildResult<()> {
        std::fs::create_dir_all(spec.work_dir)?;
        let manifest = spec.work_dir.join("AndroidManifest.xml");
        std::fs::write(
            &manifest,
            manifest_with_package(&std::fs::read_to_string(spec.manifest)?, spec.package),
        )?;
        let base = spec.work_dir.join("base.apk");
        let aligned = spec.work_dir.join("aligned.apk");

        process::run(Command::new(&self.aapt2).args(self.link_args(&manifest, &base))).await?;
        add_native_libs(&base, spec.native_libs)?;
        process::run(
            Command::new(&self.zipalign)
                .args(["-f", "-p", "4"])
                .arg(&base)
                .arg(&aligned),
        )
        .await?;

        let keystore = debug_keystore_path()?;
        if !keystore.is_file() {
            self.create_debug_keystore(&keystore).await?;
        }
        if let Some(parent) = spec.output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sign = Command::new(&self.apksigner);
        sign.args(sign_args(&keystore, spec.output, &aligned));
        if let Some(java_home) = &self.java_home {
            sign.env("JAVA_HOME", java_home);
        }
        process::run(&mut sign).await
    }

    fn link_args(&self, manifest: &Path, output: &Path) -> Vec<String> {
        vec![
            "link".into(),
            "--manifest".into(),
            manifest.display().to_string(),
            "-I".into(),
            self.android_jar.display().to_string(),
            "--min-sdk-version".into(),
            MIN_SDK.to_string(),
            "--target-sdk-version".into(),
            self.api_level.to_string(),
            "-o".into(),
            output.display().to_string(),
        ]
    }

    async fn create_debug_keystore(&self, keystore: &Path) -> BuildResult<()> {
        if let Some(parent) = keystore.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let keytool = match &self.java_home {
            Some(java_home) => java_home.join("bin").join("keytool"),
            None => PathBuf::from("keytool"),
        };
        crate::ui::debug(format!(
            "creating the Android debug keystore at {}",
            keystore.display()
        ));
        process::run(Command::new(keytool).args(keytool_args(keystore))).await
    }
}

/// `~/.android/debug.keystore`, the path Android Studio and Gradle share.
fn debug_keystore_path() -> BuildResult<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| BuildError::EnvVarError {
            var: "HOME".into(),
            reason: "not set; needed to find ~/.android/debug.keystore".into(),
        })?;
    Ok(PathBuf::from(home).join(".android").join("debug.keystore"))
}

fn keytool_args(keystore: &Path) -> Vec<String> {
    vec![
        "-genkeypair".into(),
        "-keystore".into(),
        keystore.display().to_string(),
        "-storepass".into(),
        DEBUG_PASSWORD.into(),
        "-alias".into(),
        DEBUG_ALIAS.into(),
        "-keypass".into(),
        DEBUG_PASSWORD.into(),
        "-keyalg".into(),
        "RSA".into(),
        "-keysize".into(),
        "2048".into(),
        "-validity".into(),
        "10000".into(),
        "-dname".into(),
        "CN=Android Debug,O=Android,C=US".into(),
    ]
}

fn sign_args(keystore: &Path, output: &Path, input: &Path) -> Vec<String> {
    vec![
        "sign".into(),
        "--ks".into(),
        keystore.display().to_string(),
        "--ks-pass".into(),
        format!("pass:{DEBUG_PASSWORD}"),
        "--ks-key-alias".into(),
        DEBUG_ALIAS.into(),
        "--key-pass".into(),
        format!("pass:{DEBUG_PASSWORD}"),
        "--out".into(),
        output.display().to_string(),
        input.display().to_string(),
    ]
}

/// `aapt2` wants the application id on the `<manifest>` tag; the scaffolded
/// file leaves it out because Gradle's `namespace` forbids it there.
fn manifest_with_package(manifest: &str, package: &str) -> String {
    match manifest.find("<manifest ") {
        Some(at) => {
            let insert = at + "<manifest ".len();
            format!(
                "{}package=\"{package}\" {}",
                &manifest[..insert],
                &manifest[insert..]
            )
        }
        None => manifest.to_string(),
    }
}

/// The archive entry for a native library.
fn lib_entry_name(abi: &str, lib: &Path) -> BuildResult<String> {
    let file = lib
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| BuildError::path_not_found(lib.to_path_buf(), "native library"))?;
    Ok(format!("lib/{abi}/{file}"))
}

/// Append the libraries to the archive `aapt2` produced, uncompressed.
fn add_native_libs(apk: &Path, libs: &[(String, PathBuf)]) -> BuildResult<()> {
    let zip_error =
        |error: zip::result::ZipError| BuildError::Other(format!("{}: {error}", apk.display()));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(apk)?;
    let mut writer = ZipWriter::new_append(file).map_err(zip_error)?;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (abi, lib) in libs {
        writer
            .start_file(lib_entry_name(abi, lib)?, options)
            .map_err(zip_error)?;
        std::io::copy(&mut BufReader::new(File::open(lib)?), &mut writer)?;
    }
    let mut file = writer.finish().map_err(zip_error)?;
    file.flush()?;
    Ok(())
}

/// Entry names of an archive, for tests and `flui build android -v`.
pub(crate) fn archive_entries(apk: &Path) -> BuildResult<Vec<String>> {
    let archive = ZipArchive::new(BufReader::new(File::open(apk)?))
        .map_err(|error| BuildError::Other(format!("{}: {error}", apk.display())))?;
    Ok(archive.file_names().map(str::to_string).collect())
}

/// The subdirectory with the highest version-like name.
fn newest_subdir(parent: &Path, accept: impl Fn(&str) -> bool) -> Option<PathBuf> {
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in std::fs::read_dir(parent).ok()?.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !accept(name) || !entry.path().is_dir() {
            continue;
        }
        let key: Vec<u32> = name
            .split(['.', '-'])
            .map(|part| part.parse().unwrap_or(0))
            .collect();
        if best.as_ref().is_none_or(|(k, _)| key > *k) {
            best = Some((key, entry.path()));
        }
    }
    best.map(|(_, path)| path)
}

/// `platforms/android-<N>` with the highest `N` that has an `android.jar`.
fn newest_platform(platforms: &Path) -> Option<(PathBuf, u32)> {
    let dir = newest_subdir(platforms, |name| {
        name.starts_with("android-") && platforms.join(name).join("android.jar").is_file()
    })?;
    let level = dir
        .file_name()?
        .to_str()?
        .strip_prefix("android-")?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some((dir, level))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_gets_the_package_attribute_gradle_forbids() {
        let manifest = "<?xml version=\"1.0\"?>\n<manifest xmlns:android=\"x\">\n</manifest>\n";
        let out = manifest_with_package(manifest, "com.example.app");
        assert!(out.contains("<manifest package=\"com.example.app\" xmlns:android=\"x\">"));
        assert_eq!(manifest_with_package("no tag", "x"), "no tag");
    }

    #[test]
    fn native_libraries_are_appended_stored_under_their_abi() {
        let dir = tempfile::tempdir().expect("temp");
        let apk = dir.path().join("base.apk");
        {
            let mut writer = ZipWriter::new(File::create(&apk).expect("apk"));
            writer
                .start_file("AndroidManifest.xml", SimpleFileOptions::default())
                .expect("entry");
            writer.write_all(b"<manifest/>").expect("write");
            writer.finish().expect("finish");
        }
        let lib = dir.path().join("libapp.so");
        std::fs::write(&lib, b"\x7fELF").expect("lib");
        add_native_libs(&apk, &[("arm64-v8a".into(), lib.clone())]).expect("append");
        let mut names = archive_entries(&apk).expect("entries");
        names.sort();
        assert_eq!(names, ["AndroidManifest.xml", "lib/arm64-v8a/libapp.so"]);
        let mut archive = ZipArchive::new(File::open(&apk).expect("open")).expect("zip");
        let entry = archive
            .by_name("lib/arm64-v8a/libapp.so")
            .expect("lib entry");
        assert_eq!(entry.compression(), CompressionMethod::Stored);
    }

    #[test]
    fn newest_versions_win_and_platforms_need_an_android_jar() {
        let dir = tempfile::tempdir().expect("temp");
        for name in ["33.0.2", "35.0.0", "34.0.0-rc1", "notes"] {
            std::fs::create_dir_all(dir.path().join("build-tools").join(name)).expect("dir");
        }
        let best = newest_subdir(&dir.path().join("build-tools"), |n| {
            n.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .expect("best");
        assert_eq!(best.file_name().unwrap(), "35.0.0");

        let platforms = dir.path().join("platforms");
        std::fs::create_dir_all(platforms.join("android-36")).expect("dir");
        std::fs::create_dir_all(platforms.join("android-35")).expect("dir");
        std::fs::write(platforms.join("android-35/android.jar"), b"").expect("jar");
        std::fs::create_dir_all(platforms.join("android-33-ext4")).expect("dir");
        std::fs::write(platforms.join("android-33-ext4/android.jar"), b"").expect("jar");
        let (path, level) = newest_platform(&platforms).expect("platform");
        assert_eq!(
            level, 35,
            "android-36 has no android.jar and must be skipped"
        );
        assert!(path.ends_with("android-35"));
    }

    #[test]
    fn signing_and_keytool_use_the_debug_identity() {
        let ks = Path::new("/home/u/.android/debug.keystore");
        let args = sign_args(ks, Path::new("out.apk"), Path::new("in.apk"));
        assert_eq!(args[0], "sign");
        assert!(args.contains(&"androiddebugkey".to_string()));
        assert!(args.contains(&"pass:android".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("in.apk"));
        let kt = keytool_args(ks);
        assert!(kt.contains(&"-genkeypair".to_string()));
        assert!(kt.contains(&"CN=Android Debug,O=Android,C=US".to_string()));
    }
}
