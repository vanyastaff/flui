//! An APK without Gradle: the SDK's own `build-tools` do everything a
//! `hasCode="false"` NativeActivity app needs.
//!
//! 1. `aapt2 link` turns `AndroidManifest.xml` into a base archive (no
//!    resources: the manifest carries no icon, the platform's default one
//!    applies).
//! 2. The native libraries go in as `lib/<abi>/lib<name>.so`, stored
//!    uncompressed: with `targetSdk >= 23` the platform maps them straight
//!    out of the archive instead of extracting, which needs page alignment.
//! 3. `zipalign -P 16` provides that alignment for 16 KB pages, which Google
//!    Play requires of 64-bit libraries (a 4 KB alignment fails to map on a
//!    16 KB device); `-P` needs build-tools 35.
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
    /// The `minSdk` the APK declares: FLUI's floor, or the application's
    /// when `flui.toml` raises it (ADR-0244 §10).
    pub(crate) min_sdk: u32,
}

/// The SDK pieces the packager runs, resolved once.
pub(crate) struct BuildTools {
    aapt2: PathBuf,
    zipalign: PathBuf,
    apksigner: PathBuf,
    android_jar: PathBuf,
    /// `JAVA_HOME` for `apksigner` and `keytool`, when known.
    java_home: Option<PathBuf>,
}

/// The `targetSdk` the APK declares: FLUI's pinned one (ADR-0255 decision 7),
/// never the newest installed platform, whose behaviour changes FLUI may not
/// meet.
const TARGET_SDK: u32 = crate::build::platform_floor::ANDROID_TARGET_SDK;

/// The oldest `android.jar` the APK links against: the `targetSdk` it
/// declares, since a target above the compile SDK names behaviour the
/// linker never saw.
const MIN_COMPILE_SDK: u32 = TARGET_SDK;

// The scaffolded manifest's `configChanges` names `fontWeightAdjustment`, a
// flag from API 31.
const _: () = assert!(MIN_COMPILE_SDK >= 31);

/// The oldest build-tools whose `zipalign` takes `-P 16` (16 KB pages).
const MIN_BUILD_TOOLS: u32 = 35;

/// The `zipalign` arguments that page-align `base` into `aligned`: every
/// uncompressed library at a 16 KB boundary.
fn zipalign_args(base: &Path, aligned: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "-f".into(),
        "-P".into(),
        "16".into(),
        base.as_os_str().to_owned(),
        aligned.as_os_str().to_owned(),
    ]
}
const DEBUG_ALIAS: &str = "androiddebugkey";
const DEBUG_PASSWORD: &str = "android";

impl BuildTools {
    /// The highest installed `build-tools` and `platforms` of `sdk_root`.
    pub(crate) fn locate(sdk_root: &Path, java_home: Option<PathBuf>) -> BuildResult<Self> {
        let build_tools = newest_subdir(&sdk_root.join("build-tools"), |name| {
            name.split(['.', '-'])
                .next()
                .and_then(|major| major.parse::<u32>().ok())
                .is_some_and(|major| major >= MIN_BUILD_TOOLS)
        })
        .ok_or_else(|| BuildError::ToolNotFound {
            tool: format!(
                "Android build-tools {MIN_BUILD_TOOLS} or later (aapt2, zipalign, apksigner)"
            ),
            install_hint: format!("sdkmanager \"build-tools;{TARGET_SDK}.0.0\""),
        })?;
        let (platform, _) = newest_platform(&sdk_root.join("platforms"))
            .filter(|(_, level)| *level >= MIN_COMPILE_SDK)
            .ok_or_else(|| BuildError::ToolNotFound {
                tool: format!(
                    "an Android platform (android.jar) at API {MIN_COMPILE_SDK} or later"
                ),
                install_hint: format!("sdkmanager \"platforms;android-{MIN_COMPILE_SDK}\""),
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
                    install_hint: format!("sdkmanager \"build-tools;{TARGET_SDK}.0.0\""),
                })
        };
        Ok(Self {
            aapt2: exe("aapt2")?,
            zipalign: exe("zipalign")?,
            apksigner: exe("apksigner")?,
            android_jar: platform.join("android.jar"),
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

        process::run(Command::new(&self.aapt2).args(self.link_args(
            &manifest,
            &base,
            spec.min_sdk,
        )))
        .await?;
        add_native_libs(&base, spec.native_libs)?;
        process::run(Command::new(&self.zipalign).args(zipalign_args(&base, &aligned))).await?;

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

    fn link_args(&self, manifest: &Path, output: &Path, min_sdk: u32) -> Vec<String> {
        vec![
            "link".into(),
            "--manifest".into(),
            manifest.display().to_string(),
            "-I".into(),
            self.android_jar.display().to_string(),
            "--min-sdk-version".into(),
            min_sdk.to_string(),
            "--target-sdk-version".into(),
            TARGET_SDK.to_string(),
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
    let zip_error =
        |error: zip::result::ZipError| BuildError::Other(format!("{}: {error}", apk.display()));
    let archive = ZipArchive::new(BufReader::new(File::open(apk)?)).map_err(zip_error)?;
    archive
        .file_names()
        .map(|name| name.map(String::from).map_err(zip_error))
        .collect()
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

    /// An SDK root holding the named `build-tools` (with the three tools) and
    /// `platforms/android-<N>` (with an `android.jar`).
    fn sdk(build_tools: &[&str], platforms: &[u32]) -> tempfile::TempDir {
        let sdk = tempfile::tempdir().expect("temp");
        for version in build_tools {
            let dir = sdk.path().join("build-tools").join(version);
            std::fs::create_dir_all(&dir).expect("build-tools");
            for tool in ["aapt2", "zipalign", "apksigner"] {
                let file = if cfg!(windows) {
                    format!("{tool}.exe")
                } else {
                    tool.to_owned()
                };
                std::fs::write(dir.join(file), b"").expect("tool");
            }
        }
        for level in platforms {
            let platform = sdk
                .path()
                .join("platforms")
                .join(format!("android-{level}"));
            std::fs::create_dir_all(&platform).expect("platform");
            std::fs::write(platform.join("android.jar"), b"").expect("android.jar");
        }
        sdk
    }

    fn the_link_declares_the_floor_and_the_pinned_target() {
        let newer = TARGET_SDK + 1;
        let sdk = sdk(&["36.0.0"], &[TARGET_SDK, newer]);
        let tools = BuildTools::locate(sdk.path(), None).expect("located");
        assert!(
            tools
                .android_jar
                .ends_with(format!("android-{newer}/android.jar")),
            "{}",
            tools.android_jar.display()
        );
        let args = tools.link_args(Path::new("AndroidManifest.xml"), Path::new("base.apk"), 26);
        let target = TARGET_SDK.to_string();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--min-sdk-version", "26"]),
            "{args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--target-sdk-version", target.as_str()]),
            "the newest installed platform must not become the target: {args:?}"
        );
    }

    fn a_platform_older_than_the_target_is_refused() {
        let sdk = sdk(&["36.0.0"], &[30, TARGET_SDK - 1]);
        let Err(BuildError::ToolNotFound { tool, install_hint }) =
            BuildTools::locate(sdk.path(), None)
        else {
            panic!("an android.jar below targetSdk {TARGET_SDK} was accepted");
        };
        assert!(tool.contains(&format!("API {TARGET_SDK}")), "{tool}");
        assert!(
            install_hint.contains(&format!("platforms;android-{TARGET_SDK}")),
            "{install_hint}"
        );
    }

    fn build_tools_without_16_kb_zipalign_are_refused() {
        let sdk = sdk(&["34.0.0", "30.0.3"], &[TARGET_SDK]);
        let Err(BuildError::ToolNotFound { tool, .. }) = BuildTools::locate(sdk.path(), None)
        else {
            panic!("build-tools 34, whose zipalign has no -P, were accepted");
        };
        assert!(tool.contains("build-tools 35 or later"), "{tool}");
    }

    fn libraries_are_aligned_for_16_kb_pages() {
        let args = zipalign_args(Path::new("base.apk"), Path::new("aligned.apk"));
        assert_eq!(args, ["-f", "-P", "16", "base.apk", "aligned.apk"]);
    }

    #[test]
    fn the_apk_meets_the_store_levels() {
        crate::test_cases::run_cases(&[
            (
                "the link declares the floor and the pinned target",
                the_link_declares_the_floor_and_the_pinned_target,
            ),
            (
                "a platform older than the target is refused",
                a_platform_older_than_the_target_is_refused,
            ),
            (
                "build-tools without 16 KB zipalign are refused",
                build_tools_without_16_kb_zipalign_are_refused,
            ),
            (
                "libraries are aligned for 16 KB pages",
                libraries_are_aligned_for_16_kb_pages,
            ),
        ]);
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
        add_native_libs(&apk, &[("arm64-v8a".into(), lib)]).expect("append");
        let mut names = archive_entries(&apk).expect("entries");
        names.sort();
        assert_eq!(names, ["AndroidManifest.xml", "lib/arm64-v8a/libapp.so"]);
        let mut archive = ZipArchive::new(File::open(&apk).expect("open")).expect("zip");
        let entry = archive
            .by_name("lib/arm64-v8a/libapp.so")
            .expect("lib entry");
        assert_eq!(entry.compression(), CompressionMethod::Stored);
    }
}
