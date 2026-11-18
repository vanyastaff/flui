//! The Android providers: the Storage Access Framework's pickers, `Intent`s
//! and an `AlertDialog`, called through JNI over the Activity the host
//! lends.
//!
//! Android's own classes are called from Rust; the crate ships Java only
//! where JNI cannot stand in for it (`android/java`, `dev.flui.system`):
//!
//! - `FluiDocuments`, the content provider a view or a share hands a file
//!   through: Android instantiates a provider by the class name the
//!   manifest states.
//! - `FluiCallback`, the handler of a `java.lang.reflect.Proxy`: JNI can
//!   define no class, and a callback Android makes (a `Runnable` on its main
//!   thread, a dialog's listeners) needs an object of a Java type. One
//!   handler serves every interface, and its calls arrive at one native
//!   method.
//!
//! Each service runs where Android puts it:
//!
//! - **On the owner thread**, which on Android is the thread `android_main`
//!   runs on, not Java's main thread: what a request becomes, the `Intent`
//!   of a picker (filled inside the host's call, in the frame it lends), and
//!   the hand-over of an answer the host lent.
//! - **On Java's main thread**: starting an activity (a view, the share
//!   sheet) and the message box, each a job the library posts there
//!   (`Activity.runOnUiThread`).
//! - **On a thread of its own** ([`workers`]): whatever reads another
//!   application's content provider (a picked document's name, size and
//!   contents, a save's write) or the file system (the file a launch or a
//!   share hands over), since a provider answers when it will.
//!
//! The native methods go through `owner::android::native_call`, which `cargo
//! xtask panic-boundary` holds them to. The rules of each provider (what a
//! request becomes, what an answer means) are tested beside it on every
//! host, the dialogs' whole path over a stand-in Activity; the JNI calls
//! compile on Android only, and no compiler checks the names and signatures
//! they spell.

pub(crate) mod documents;
#[cfg(target_os = "android")]
#[expect(
    unsafe_code,
    reason = "the frames the host lends, the callback handles Java holds, RegisterNatives"
)]
mod java;
mod launcher;
mod message_box;
mod os;
mod share;
mod user_dirs;
mod workers;

use flui_platform::SystemFailure;

/// `FluiCallback.PROTOCOL`: the contract between the library and its Java
/// classes, checked before anything is bound or called.
pub(crate) const PROTOCOL: i32 = 1;

/// The binary name of the class Android calls the library back through.
pub(crate) const CALLBACK_CLASS: &str = "dev.flui.system.FluiCallback";

/// The binary name of the content provider.
pub(crate) const DOCUMENTS_CLASS: &str = "dev.flui.system.FluiDocuments";

/// What starting an activity on Java's main thread did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartAnswer {
    /// The activity started.
    Started,
    /// No installed application handles the intent
    /// (`ActivityNotFoundException`).
    NoHandler,
    /// The application may not start it (`SecurityException`).
    Denied,
    /// Android failed otherwise.
    Failed,
}

/// The failure of a start Android gives no code for: an exception its
/// documentation does not name, so an answer outside its contract.
pub(crate) const UNNAMED_FAILURE: SystemFailure = SystemFailure::MalformedAnswer;

#[cfg(target_os = "android")]
pub(crate) use java::register;

/// The Java source of the class named `binary_name`, from the sources the
/// checked-in dex is compiled from (`cargo xtask android-dex` holds the dex
/// to them).
#[cfg(test)]
pub(crate) fn java_source(binary_name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("android/java")
        .join(format!("{}.java", binary_name.replace('.', "/")));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{CALLBACK_CLASS, DOCUMENTS_CLASS, PROTOCOL, java_source};

    /// The library and its Java classes state one protocol and the native
    /// methods the library binds, and the provider the manifest declares is
    /// the class and the authority the Java source serves.
    #[test]
    fn the_java_classes_state_what_the_library_and_the_manifest_name() {
        let callback = java_source(CALLBACK_CLASS);
        for stated in [
            format!("public static final int PROTOCOL = {PROTOCOL};"),
            "FluiCallback(long handle) {".to_owned(),
            "private static native void called(long handle, String method, int argument);"
                .to_owned(),
            "private static native void released(long handle);".to_owned(),
        ] {
            assert!(callback.contains(&stated), "FluiCallback states `{stated}`");
        }

        let documents = java_source(DOCUMENTS_CLASS);
        let manifest: toml::Table = include_str!("../../Cargo.toml")
            .parse()
            .expect("the crate's manifest is TOML");
        let providers = manifest["package"]["metadata"]["flui"]["declares"]["android"]["providers"]
            .as_array()
            .expect("the crate declares its provider");
        let [provider] = providers.as_slice() else {
            panic!("the crate declares one provider: {providers:?}");
        };
        assert_eq!(provider["class"].as_str(), Some(DOCUMENTS_CLASS));
        let authority = provider["authority"].as_str().expect("an authority");
        assert!(
            documents.contains(&format!(
                "static final String AUTHORITY_SUFFIX = \".{authority}\";"
            )),
            "FluiDocuments serves the authority the manifest declares: {authority}"
        );
        for stated in [
            "static Uri offer(Context context, int fd, String name) {",
            "static String typeOfName(String name) {",
        ] {
            assert!(
                documents.contains(stated),
                "FluiDocuments states `{stated}`"
            );
        }
    }
}
