//! The Android providers: the Storage Access Framework's pickers, `Intent`s
//! and an `AlertDialog`, called through JNI over the Activity the host
//! lends.
//!
//! Android's own classes are called from Rust; the crate ships Java only as
//! the leaf ADR-0377 allows (`android/java`, `dev.flui.system`), as a
//! checked-in dex its declarations name:
//!
//! - `FluiDocuments`, the content provider a view or a share hands a file
//!   through: a component the manifest must name, which Android instantiates
//!   by that name.
//! - `FluiCallback`, the one handler of every `java.lang.reflect.Proxy` the
//!   crate makes for a listener interface (a dialog's three listeners): JNI
//!   defines no class, and a handler is an object of a Java type. Its calls
//!   arrive at one native method.
//!
//! Each service runs where Android puts it:
//!
//! - **On the owner thread**, which on Android is the thread `android_main`
//!   runs on, not Java's main thread: what a request becomes, the `Intent`
//!   of a picker (filled inside the host's call, in the frame it lends), and
//!   the hand-over of an answer the host lent.
//! - **On Java's main thread**: starting an activity (a view, the share
//!   sheet) and the message box, each work the host runs there for the
//!   provider (`Activity::on_main_thread`).
//! - **On a thread of its own** ([`workers`]): whatever reads another
//!   application's content provider (a picked document's name, size and
//!   contents, a save's write) or the file system (the file a launch or a
//!   share hands over), since a provider answers when it will.
//!
//! The native methods go through `owner::android::native_call`, the crate's
//! panic boundary for a JNI entry (ADR-0151 §16). The rules of each provider (what a
//! request becomes, what an answer means) are tested beside it on every
//! host, the dialogs' whole path over a stand-in Activity; the JNI calls
//! compile on Android only, and no compiler checks the names and signatures
//! they spell.

pub(crate) mod documents;
mod grants;
#[cfg(target_os = "android")]
#[expect(
    unsafe_code,
    reason = "the frames the host lends, the callback handles Java holds, RegisterNatives"
)]
mod java;
mod launcher;
mod message_box;
mod opened;
mod os;
mod share;
mod user_dirs;
mod workers;

use flui_platform::SystemFailure;

/// `FluiCallback.PROTOCOL`: the contract between the library and its Java
/// classes, checked before anything is bound or called.
pub(crate) const PROTOCOL: i32 = 3;

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
    /// `FluiDocuments` serves `MAX_OFFERED` documents already: its `offer`
    /// refused one more (`IllegalStateException`, classified from that call
    /// alone). One must be opened, withdrawn or expire first.
    TooManyOffered,
    /// Android failed otherwise.
    Failed,
}

impl StartAnswer {
    /// What a start answers when `FluiDocuments.offer` threw while its
    /// `Intent` was built: `IllegalStateException` (`is_illegal_state`), the
    /// one exception `offer` throws by contract, is the provider full;
    /// anything else is a failure. Only `offer`'s own exception is judged
    /// here, never one `startActivity` threw.
    pub(crate) const fn of_offer_exception(is_illegal_state: bool) -> Self {
        if is_illegal_state {
            Self::TooManyOffered
        } else {
            Self::Failed
        }
    }

    /// Whether the offers made for the start stay served: only a start that
    /// happened hands their grant to anyone. Every other answer withdraws
    /// them at once, so a refused start holds none of `MAX_OFFERED`.
    pub(crate) const fn keeps_offers(self) -> bool {
        matches!(self, Self::Started)
    }
}

/// The failure of a start Android gives no code for: an exception its
/// documentation does not name, so an answer outside its contract.
pub(crate) const UNNAMED_FAILURE: SystemFailure = SystemFailure::MalformedAnswer;

/// What a start whose files `FluiDocuments` could not offer answers:
/// `EMFILE`, since the provider holds a descriptor per offer and has as many
/// as it keeps. Trying again succeeds once an offer was opened, withdrawn or
/// expired.
pub(crate) const TOO_MANY_OFFERED: SystemFailure = SystemFailure::Os(24);

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
    /// methods the library binds; the provider the manifest declares is the
    /// class the Java source defines, and it serves under the authority
    /// Android attached it with rather than one it computes.
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
        assert!(
            provider["authority"].as_str().is_some(),
            "the declaration names an authority suffix"
        );
        assert!(
            !documents.contains("getPackageName()"),
            "FluiDocuments takes its authority from the manifest, not the package name"
        );
        for stated in [
            "public final class FluiDocuments extends ContentProvider {",
            "public void attachInfo(Context context, ProviderInfo info) {",
            "attachedAuthority = info.authority.split(\";\")[0];",
            ".authority(authority)",
            "static String authority() {",
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
