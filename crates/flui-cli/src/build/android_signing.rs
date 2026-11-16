//! Which key signs an Android package, decided once for both packagers.
//!
//! - A debug build is signed with the SDK user's debug key, the
//!   `debug.keystore` Android Studio and the Android Gradle plugin create and
//!   share, found where they find it ([`debug_keystore`]).
//! - A release build is signed with the key `flui.toml` names:
//!
//!   ```toml
//!   [signing.android]
//!   keystore = "keys/upload.jks"   # relative to flui.toml
//!   alias = "upload"
//!   ```
//!
//!   Its passwords come from the environment, [`STORE_PASSWORD_ENV`] and,
//!   when the key's differs, [`KEY_PASSWORD_ENV`]; no file and no command
//!   line carries them: `apksigner` reads them with `env:` and Gradle with
//!   `providers.environmentVariable`.
//! - A release build of an application that names no key is signed with the
//!   debug key, as Flutter's template does, so `flui run --release` still
//!   installs; [`SigningKey::is_debug_for_release`] tells the build to say so,
//!   since a store refuses such a package.
//!
//! The packager without Gradle passes the key to `apksigner`
//! ([`SigningKey::apksigner_args`]); a Gradle project receives it in
//! `gradle.properties` (`flui.signing.storeFile`, `flui.signing.keyAlias`),
//! which the scaffolded `app/build.gradle.kts` reads into its release
//! signing config.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::build::platform::Profile;

/// The environment variable holding the release keystore's password.
pub(crate) const STORE_PASSWORD_ENV: &str = "FLUI_ANDROID_KEYSTORE_PASSWORD";
/// The environment variable holding the release key's password, when it is
/// not the keystore's.
pub(crate) const KEY_PASSWORD_ENV: &str = "FLUI_ANDROID_KEY_PASSWORD";

/// The alias of the key in every SDK user's debug keystore.
pub(crate) const DEBUG_ALIAS: &str = "androiddebugkey";
/// The password of the debug keystore and of its key, which the SDK
/// publishes: it protects nothing.
pub(crate) const DEBUG_PASSWORD: &str = "android";

/// `[signing.android]` in `flui.toml`: the key a release is signed with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AndroidReleaseKey {
    /// The keystore (JKS or PKCS #12), relative to the directory of
    /// `flui.toml` unless absolute.
    pub(crate) keystore: PathBuf,
    /// The alias of the key in it.
    pub(crate) alias: String,
}

/// The key a build signs its package with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SigningKey {
    /// The SDK user's debug key.
    Debug {
        /// Its keystore, which may not exist yet.
        keystore: PathBuf,
        /// Whether the build is a release one, signed with this key only
        /// because the application names no release key.
        for_release: bool,
    },
    /// The application's release key.
    Release {
        /// The keystore, which exists.
        keystore: PathBuf,
        /// The alias of the key.
        alias: String,
        /// The variable the key's password is read from: [`KEY_PASSWORD_ENV`]
        /// when set, the keystore's otherwise.
        key_password_env: &'static str,
    },
}

/// Why a package cannot be signed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AndroidSigningError {
    /// No variable names the user's home, so the debug keystore has no place.
    #[error(
        "no ANDROID_USER_HOME, {home} or HOME is set, so the debug keystore has no place; set \
         ANDROID_USER_HOME to the directory that holds debug.keystore"
    )]
    NoUserHome {
        /// The variable Java reads the user's home from on this OS.
        home: &'static str,
    },
    /// The release keystore `flui.toml` names is not a file.
    #[error(
        "flui.toml: [signing.android] keystore {} is not a file",
        .keystore.display()
    )]
    KeystoreMissing { keystore: PathBuf },
    /// `flui.toml` names a release key with an empty alias.
    #[error("flui.toml: [signing.android] alias is empty")]
    EmptyAlias,
    /// The release keystore's password is not in the environment.
    #[error(
        "flui.toml names a release key, but {STORE_PASSWORD_ENV} is not set; set it to the \
         keystore's password (and {KEY_PASSWORD_ENV} to the key's, when it differs)"
    )]
    PasswordUnset,
}

impl SigningKey {
    /// The key a build with `profile` signs with.
    ///
    /// `release_key` is `flui.toml`'s `[signing.android]`, its keystore
    /// relative to `project_dir`; `env` reads an environment variable and
    /// `is_dir` whether a path is a directory, so a test states both.
    ///
    /// # Errors
    ///
    /// A release key whose keystore is not a file, whose alias is empty or
    /// whose password is not in the environment; a debug key with no home to
    /// keep its keystore in.
    pub(crate) fn for_build(
        profile: Profile,
        release_key: Option<&AndroidReleaseKey>,
        project_dir: &Path,
        env: &dyn Fn(&str) -> Option<OsString>,
        is_dir: &dyn Fn(&Path) -> bool,
    ) -> Result<Self, AndroidSigningError> {
        let release_key = match profile {
            Profile::Release => release_key,
            Profile::Debug => None,
        };
        let Some(release_key) = release_key else {
            return Ok(Self::Debug {
                keystore: debug_keystore(env, is_dir, cfg!(windows))?,
                for_release: profile == Profile::Release,
            });
        };
        if release_key.alias.trim().is_empty() {
            return Err(AndroidSigningError::EmptyAlias);
        }
        let keystore = project_dir.join(&release_key.keystore);
        if !keystore.is_file() {
            return Err(AndroidSigningError::KeystoreMissing { keystore });
        }
        let set = |name: &str| env(name).is_some_and(|value| !value.is_empty());
        if !set(STORE_PASSWORD_ENV) {
            return Err(AndroidSigningError::PasswordUnset);
        }
        Ok(Self::Release {
            keystore,
            alias: release_key.alias.clone(),
            key_password_env: if set(KEY_PASSWORD_ENV) {
                KEY_PASSWORD_ENV
            } else {
                STORE_PASSWORD_ENV
            },
        })
    }

    /// The keystore the key is in.
    pub(crate) fn keystore(&self) -> &Path {
        match self {
            Self::Debug { keystore, .. } | Self::Release { keystore, .. } => keystore,
        }
    }

    /// Whether this is a release build signed with the debug key, which a
    /// store refuses.
    pub(crate) fn is_debug_for_release(&self) -> bool {
        matches!(
            self,
            Self::Debug {
                for_release: true,
                ..
            }
        )
    }

    /// The release keystore and alias a Gradle project's release signing
    /// config reads, or `None` when the build signs with the debug key.
    pub(crate) fn release_key(&self) -> Option<(&Path, &str)> {
        match self {
            Self::Release {
                keystore, alias, ..
            } => Some((keystore, alias)),
            Self::Debug { .. } => None,
        }
    }

    /// The `apksigner sign` arguments that select this key. A release key's
    /// passwords are named by their variables (`env:`), never written: only
    /// the debug password, which the SDK publishes, appears.
    pub(crate) fn apksigner_args(&self) -> Vec<OsString> {
        let (keystore, store_password, alias, key_password) = match self {
            Self::Debug { keystore, .. } => (
                keystore,
                format!("pass:{DEBUG_PASSWORD}"),
                DEBUG_ALIAS,
                format!("pass:{DEBUG_PASSWORD}"),
            ),
            Self::Release {
                keystore,
                alias,
                key_password_env,
            } => (
                keystore,
                format!("env:{STORE_PASSWORD_ENV}"),
                alias.as_str(),
                format!("env:{key_password_env}"),
            ),
        };
        vec![
            "--ks".into(),
            keystore.as_os_str().to_owned(),
            "--ks-pass".into(),
            store_password.into(),
            "--ks-key-alias".into(),
            alias.into(),
            "--key-pass".into(),
            key_password.into(),
        ]
    }
}

/// The debug keystore Android Studio and the Android Gradle plugin use:
/// `debug.keystore` in the Android preferences directory, found as their
/// `AbstractAndroidLocations` finds it.
///
/// 1. `ANDROID_USER_HOME`, the directory itself.
/// 2. `ANDROID_PREFS_ROOT`, then `ANDROID_SDK_HOME`, each an existing
///    directory, with `.android` appended; an `ANDROID_SDK_HOME` that holds an
///    SDK (`platforms` and `platform-tools`, no `.android`) is a mistaken
///    setting and is skipped.
/// 3. Java's `user.home` with `.android` appended: the profile directory on
///    Windows (`USERPROFILE`), `HOME` elsewhere; then `HOME`.
///
/// # Errors
///
/// [`AndroidSigningError::NoUserHome`] when none of them is set.
pub(crate) fn debug_keystore(
    env: &dyn Fn(&str) -> Option<OsString>,
    is_dir: &dyn Fn(&Path) -> bool,
    windows: bool,
) -> Result<PathBuf, AndroidSigningError> {
    let path = |name: &str| {
        env(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let preferences = path("ANDROID_USER_HOME")
        .or_else(|| {
            ["ANDROID_PREFS_ROOT", "ANDROID_SDK_HOME"]
                .into_iter()
                .filter_map(|name| path(name).map(|root| (name, root)))
                .find(|(name, root)| {
                    is_dir(root)
                        && !(*name == "ANDROID_SDK_HOME"
                            && is_dir(&root.join("platforms"))
                            && is_dir(&root.join("platform-tools"))
                            && !is_dir(&root.join(".android")))
                })
                .map(|(_, root)| root.join(".android"))
        })
        .or_else(|| {
            let user_home = if windows { "USERPROFILE" } else { "HOME" };
            path(user_home)
                .or_else(|| path("HOME"))
                .map(|home| home.join(".android"))
        })
        .ok_or(AndroidSigningError::NoUserHome {
            home: if windows { "USERPROFILE" } else { "HOME" },
        })?;
    Ok(preferences.join("debug.keystore"))
}

/// The `keytool` arguments that create the debug keystore at `keystore`, as
/// the Android Gradle plugin creates it.
pub(crate) fn debug_keystore_creation_args(keystore: &Path) -> Vec<&OsStr> {
    let mut args: Vec<&OsStr> = ["-genkeypair", "-keystore"]
        .into_iter()
        .map(OsStr::new)
        .collect();
    args.push(keystore.as_os_str());
    args.extend(
        [
            "-storepass",
            DEBUG_PASSWORD,
            "-alias",
            DEBUG_ALIAS,
            "-keypass",
            DEBUG_PASSWORD,
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "10000",
            "-dname",
            "CN=Android Debug,O=Android,C=US",
        ]
        .into_iter()
        .map(OsStr::new),
    );
    args
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{
        AndroidReleaseKey, AndroidSigningError, KEY_PASSWORD_ENV, STORE_PASSWORD_ENV, SigningKey,
        debug_keystore,
    };
    use crate::build::platform::Profile;

    /// An environment of `vars` and a file system whose directories are
    /// `dirs`.
    struct World {
        vars: BTreeMap<&'static str, &'static str>,
        dirs: BTreeSet<PathBuf>,
    }

    impl World {
        fn new(vars: &[(&'static str, &'static str)], dirs: &[&str]) -> Self {
            Self {
                vars: vars.iter().copied().collect(),
                dirs: dirs.iter().map(PathBuf::from).collect(),
            }
        }

        fn env(&self) -> impl Fn(&str) -> Option<OsString> + '_ {
            |name| self.vars.get(name).map(OsString::from)
        }

        fn is_dir(&self) -> impl Fn(&Path) -> bool + '_ {
            |path| self.dirs.contains(path)
        }

        fn debug_keystore(&self, windows: bool) -> Result<PathBuf, AndroidSigningError> {
            debug_keystore(&self.env(), &self.is_dir(), windows)
        }
    }

    /// The debug keystore is where the Android Gradle plugin keeps it, so a
    /// package from either packager installs over the other's.
    #[test]
    fn the_debug_keystore_is_where_the_android_gradle_plugin_keeps_it() {
        let keystore = |dir: &str| Ok(Path::new(dir).join("debug.keystore"));
        let cases: [(&str, World, bool, Result<PathBuf, AndroidSigningError>); 8] = [
            (
                "ANDROID_USER_HOME is the directory itself, existing or not",
                World::new(&[("ANDROID_USER_HOME", "/prefs"), ("HOME", "/home/u")], &[]),
                false,
                keystore("/prefs"),
            ),
            (
                "ANDROID_PREFS_ROOT is its parent",
                World::new(
                    &[("ANDROID_PREFS_ROOT", "/root"), ("HOME", "/home/u")],
                    &["/root"],
                ),
                false,
                keystore("/root/.android"),
            ),
            (
                "an ANDROID_PREFS_ROOT that does not exist is skipped",
                World::new(&[("ANDROID_PREFS_ROOT", "/gone"), ("HOME", "/home/u")], &[]),
                false,
                keystore("/home/u/.android"),
            ),
            (
                "ANDROID_SDK_HOME is its parent",
                World::new(
                    &[("ANDROID_SDK_HOME", "/sdkhome"), ("HOME", "/home/u")],
                    &["/sdkhome"],
                ),
                false,
                keystore("/sdkhome/.android"),
            ),
            (
                "an ANDROID_SDK_HOME that holds an SDK is skipped",
                World::new(
                    &[("ANDROID_SDK_HOME", "/sdk"), ("HOME", "/home/u")],
                    &["/sdk", "/sdk/platforms", "/sdk/platform-tools"],
                ),
                false,
                keystore("/home/u/.android"),
            ),
            (
                "Java's home on Windows is the profile, not HOME",
                World::new(
                    &[("HOME", "/msys/home/u"), ("USERPROFILE", "C:/Users/u")],
                    &[],
                ),
                true,
                keystore("C:/Users/u/.android"),
            ),
            (
                "HOME elsewhere",
                World::new(&[("HOME", "/home/u"), ("USERPROFILE", "/other")], &[]),
                false,
                keystore("/home/u/.android"),
            ),
            (
                "no home at all",
                World::new(&[], &[]),
                true,
                Err(AndroidSigningError::NoUserHome {
                    home: "USERPROFILE",
                }),
            ),
        ];
        let failures: Vec<String> = cases
            .into_iter()
            .filter_map(|(label, world, windows, expected)| {
                let found = world.debug_keystore(windows);
                (found != expected).then(|| format!("{label}: {found:?}"))
            })
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// A debug build signs with the debug key; a release one with the key
    /// `flui.toml` names, its passwords named by variable and never written,
    /// or with the debug key, said to be one, when it names none; a release
    /// key that cannot sign is refused before anything is packaged.
    #[test]
    fn a_release_is_signed_with_the_named_key_or_says_it_is_not() {
        let project = tempfile::tempdir().expect("project");
        let keystore = project.path().join("upload.jks");
        std::fs::write(&keystore, b"a keystore").expect("keystore");
        let named = AndroidReleaseKey {
            keystore: PathBuf::from("upload.jks"),
            alias: "upload".to_owned(),
        };
        let password = "a-throwaway-password";
        let sign = |profile, key: Option<&AndroidReleaseKey>, vars: &[(&str, &str)]| {
            let vars: BTreeMap<String, String> = vars
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .chain([("HOME".to_owned(), "/home/u".to_owned())])
                .chain([("USERPROFILE".to_owned(), "/home/u".to_owned())])
                .collect();
            SigningKey::for_build(
                profile,
                key,
                project.path(),
                &|name| vars.get(name).map(OsString::from),
                &|_| false,
            )
        };
        let debug_keystore = PathBuf::from("/home/u/.android/debug.keystore");
        let mut failures = Vec::new();

        match sign(
            Profile::Debug,
            Some(&named),
            &[(STORE_PASSWORD_ENV, password)],
        ) {
            Ok(key @ SigningKey::Debug { .. })
                if key.keystore() == debug_keystore && !key.is_debug_for_release() =>
            {
                let args = key.apksigner_args();
                if args
                    .windows(2)
                    .all(|pair| pair[0] != "--ks-key-alias" || pair[1] != "androiddebugkey")
                {
                    failures.push(format!("debug alias: {args:?}"));
                }
            }
            other => failures.push(format!("a debug build: {other:?}")),
        }
        match sign(Profile::Release, None, &[]) {
            Ok(key) if key.is_debug_for_release() && key.release_key().is_none() => {}
            other => failures.push(format!("a release naming no key: {other:?}")),
        }
        match sign(
            Profile::Release,
            Some(&named),
            &[(STORE_PASSWORD_ENV, password)],
        ) {
            Ok(key) => {
                let args = key.apksigner_args();
                let expected: Vec<OsString> = [
                    keystore.as_os_str(),
                    "--ks-pass".as_ref(),
                    "env:FLUI_ANDROID_KEYSTORE_PASSWORD".as_ref(),
                    "--ks-key-alias".as_ref(),
                    "upload".as_ref(),
                    "--key-pass".as_ref(),
                    "env:FLUI_ANDROID_KEYSTORE_PASSWORD".as_ref(),
                ]
                .into_iter()
                .map(OsString::from)
                .collect();
                if args.get(1..) != Some(expected.as_slice())
                    || key.release_key() != Some((keystore.as_path(), "upload"))
                    || args
                        .iter()
                        .any(|arg| arg.to_string_lossy().contains(password))
                {
                    failures.push(format!("a named key: {args:?}"));
                }
            }
            other => failures.push(format!("a named key: {other:?}")),
        }
        match sign(
            Profile::Release,
            Some(&named),
            &[
                (STORE_PASSWORD_ENV, password),
                (KEY_PASSWORD_ENV, "another-throwaway"),
            ],
        ) {
            Ok(key)
                if key
                    .apksigner_args()
                    .ends_with(&["--key-pass".into(), "env:FLUI_ANDROID_KEY_PASSWORD".into()]) => {}
            other => failures.push(format!("a key with its own password: {other:?}")),
        }
        for (label, key, vars, expected) in [
            (
                "no password",
                named.clone(),
                vec![],
                AndroidSigningError::PasswordUnset,
            ),
            (
                "an empty password",
                named,
                vec![(STORE_PASSWORD_ENV, "")],
                AndroidSigningError::PasswordUnset,
            ),
            (
                "no keystore",
                AndroidReleaseKey {
                    keystore: PathBuf::from("missing.jks"),
                    alias: "upload".to_owned(),
                },
                vec![(STORE_PASSWORD_ENV, password)],
                AndroidSigningError::KeystoreMissing {
                    keystore: project.path().join("missing.jks"),
                },
            ),
            (
                "an empty alias",
                AndroidReleaseKey {
                    keystore: PathBuf::from("upload.jks"),
                    alias: " ".to_owned(),
                },
                vec![(STORE_PASSWORD_ENV, password)],
                AndroidSigningError::EmptyAlias,
            ),
        ] {
            match sign(Profile::Release, Some(&key), &vars) {
                Err(error) if error == expected => {}
                other => failures.push(format!("{label}: {other:?}")),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
