//! The handshake between this library and FLUI's Java classes in the APK
//! (ADR-0255 decisions 1 and 5).
//!
//! The classes (`FluiActivity`, `FluiHost`, `FluiView` and their helpers) are
//! compiled from `crates/flui-native/android/java` into the checked-in dex,
//! which `flui build` packs into the APK from the `flui-native` the
//! application's lockfile resolves; the generated manifest names
//! `FluiActivity`. A packager that bypasses either can put another FLUI
//! version's classes next to this library, or none. So before the platform
//! starts, and so before any native method is bound, the owner reads through
//! the Activity's class loader whether the APK has `FluiHost`, the protocol it
//! states and whether the Activity is a `FluiActivity`, and [`check`] turns
//! anything else into a [`PlatformError`] naming what it found.

use crate::PlatformError;

/// The version of the contract between this library and the Java classes:
/// the native methods and their signatures, the methods the library calls,
/// and what each means. `FluiHost.PROTOCOL` states it on the Java side; the
/// two are raised together.
pub(crate) const PROTOCOL: i32 = 1;

/// The binary name of the Activity class the manifest names.
pub(crate) const ACTIVITY_CLASS: &str = "dev.flui.platform.FluiActivity";

/// The binary name of the class that states the protocol.
pub(crate) const HOST_CLASS: &str = "dev.flui.platform.FluiHost";

/// What the Activity's class loader found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    /// The class of the running Activity.
    pub(crate) activity: String,
    /// Whether that Activity is a `FluiActivity`, or a subclass of it.
    pub(crate) is_flui_activity: bool,
    /// `FluiHost.PROTOCOL`, or `None` when the loader has no `FluiHost`.
    pub(crate) protocol: Option<i32>,
}

/// Whether the platform may start over the classes `found` describes.
///
/// # Errors
///
/// [`PlatformError::HostProtocol`] when the classes state another protocol,
/// and [`PlatformError::Init`] when the APK has none of FLUI's classes or its
/// Activity is not a `FluiActivity`.
pub(crate) fn check(found: &Found) -> Result<(), PlatformError> {
    let Some(packaged) = found.protocol else {
        return Err(PlatformError::Init {
            message: format!(
                "the APK has no {HOST_CLASS}, so none of FLUI's Java classes: package the app \
                 with `flui build`, which puts flui-native's classes.dex into the APK and names \
                 {ACTIVITY_CLASS} in the manifest (ADR-0255)"
            ),
        });
    };
    if packaged != PROTOCOL {
        return Err(PlatformError::HostProtocol {
            library: PROTOCOL,
            packaged,
        });
    }
    if !found.is_flui_activity {
        return Err(PlatformError::Init {
            message: format!(
                "the manifest names {} as the Activity, which is not a {ACTIVITY_CLASS}; FLUI \
                 receives the Activity's callbacks through it, and `flui build` generates a \
                 manifest that names it (ADR-0255)",
                found.activity
            ),
        });
    }
    Ok(())
}

/// Read [`Found`] through the Activity's class loader and [`check`] it. On
/// whatever thread starts the platform, attached to the VM for the call.
///
/// # Errors
///
/// As [`check`], and [`PlatformError::Init`] when the classes cannot be read.
#[cfg(target_os = "android")]
pub(crate) fn verify(app: &android_activity::AndroidApp) -> Result<(), PlatformError> {
    let found = crate::owner::android::activity::with_activity(app, |env, activity| {
        env.with_local_frame(16, |env| read(env, activity))
    })
    .map_err(|error| PlatformError::Init {
        message: format!(
            "reading FLUI's Java classes through the Activity's class loader: {error}"
        ),
    })?;
    check(&found)
}

#[cfg(target_os = "android")]
fn read(
    env: &mut jni::Env<'_>,
    activity: &jni::objects::JObject<'_>,
) -> jni::errors::Result<Found> {
    use jni::{jni_sig, jni_str};

    let class = env.get_object_class(activity)?;
    let name = env
        .call_method(
            &class,
            jni_str!("getName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let activity_name = env
        .cast_local::<jni::objects::JString<'_>>(name)?
        .try_to_string(env)?;
    let loader = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let Some(host) = load_class(env, &loader, HOST_CLASS)? else {
        return Ok(Found {
            activity: activity_name,
            is_flui_activity: false,
            protocol: None,
        });
    };
    let protocol = env
        .get_static_field(&host, jni_str!("PROTOCOL"), jni_sig!("I"))?
        .i()?;
    let is_flui_activity = match load_class(env, &loader, ACTIVITY_CLASS)? {
        Some(flui_activity) => env.is_instance_of(activity, &flui_activity)?,
        None => false,
    };
    Ok(Found {
        activity: activity_name,
        is_flui_activity,
        protocol: Some(protocol),
    })
}

/// The class `loader` finds by the binary name `name`, or `None` when it
/// throws (`ClassNotFoundException`); the exception is cleared.
#[cfg(target_os = "android")]
pub(crate) fn load_class<'local>(
    env: &mut jni::Env<'local>,
    loader: &jni::objects::JObject<'_>,
    name: &str,
) -> jni::errors::Result<Option<jni::objects::JClass<'local>>> {
    use jni::{JValue, jni_sig, jni_str};

    let binary_name = env.new_string(name)?;
    let loaded = env.call_method(
        loader,
        jni_str!("loadClass"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
        &[JValue::Object(&binary_name)],
    );
    match loaded {
        Ok(class) => {
            let class = class.l()?;
            Ok(Some(env.cast_local::<jni::objects::JClass<'_>>(class)?))
        }
        Err(jni::errors::Error::JavaException) => {
            env.exception_clear();
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{ACTIVITY_CLASS, Found, HOST_CLASS, PROTOCOL, check};
    use crate::PlatformError;

    /// The Java source of the class named `binary_name`, from the sources the
    /// checked-in dex is compiled from (`cargo xtask android-dex` holds the
    /// dex to them).
    fn java_source(binary_name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("android/java")
            .join(format!("{}.java", binary_name.replace('.', "/")));
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    fn found(protocol: Option<i32>, is_flui_activity: bool) -> Found {
        Found {
            activity: "com.example.MainActivity".to_owned(),
            is_flui_activity,
            protocol,
        }
    }

    /// The classes this library is built with state its protocol, and the
    /// Activity the manifest names is one of them, extending `NativeActivity`.
    #[test]
    fn the_java_classes_state_the_protocol_of_the_library() {
        let host = java_source(HOST_CLASS);
        let stated = format!("public static final int PROTOCOL = {PROTOCOL};");
        assert!(host.contains(&stated), "{stated:?} in\n{host}");
        let activity = java_source(ACTIVITY_CLASS);
        assert!(
            activity.contains("public class FluiActivity extends NativeActivity"),
            "{activity}"
        );
    }

    /// Each way the APK's classes can differ from the library is refused,
    /// naming what was found; only matching classes under a `FluiActivity`
    /// start.
    #[test]
    fn the_platform_starts_only_over_its_own_java_classes() {
        assert!(check(&found(Some(PROTOCOL), true)).is_ok());

        let other = PROTOCOL + 1;
        match check(&found(Some(other), true)) {
            Err(PlatformError::HostProtocol { library, packaged }) => {
                assert_eq!((library, packaged), (PROTOCOL, other));
            }
            outcome => panic!("another protocol: {outcome:?}"),
        }
        let message = check(&found(Some(other), true))
            .expect_err("another protocol")
            .to_string();
        assert!(
            message.contains(&format!("protocol {other}"))
                && message.contains(&format!("library {PROTOCOL}")),
            "{message}"
        );

        let missing = check(&found(None, false))
            .expect_err("no classes")
            .to_string();
        assert!(
            missing.contains(HOST_CLASS) && missing.contains("flui build"),
            "{missing}"
        );

        let activity = check(&found(Some(PROTOCOL), false))
            .expect_err("another Activity")
            .to_string();
        assert!(
            activity.contains("com.example.MainActivity") && activity.contains(ACTIVITY_CLASS),
            "{activity}"
        );
    }
}
