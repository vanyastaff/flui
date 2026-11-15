//! Registering the URL schemes and the file types an unpackaged Windows
//! application opens (ADR-0307 §4): after the
//! main window's first frame, the run writes each declared scheme's key and
//! each file type's ProgID under the user's classes on the IO pool
//! (`flui_native::associations`), so a link or a file launches the
//! executable that runs, wherever it was installed or unpacked.
//!
//! A release build registers by itself; a development build registers when
//! `flui run --register-associations` asked for it, so `cargo run` of a
//! checkout does not claim a scheme or a file type for a build directory.
//! The application's own
//! [`register_associations`](crate::Application::register_associations)
//! overrides both. A packaged (MSIX) application registers through its
//! package manifest and never here.
//!
//! An uninstaller removes what registration wrote by running the executable
//! with `--flui-unregister-associations`: the run deletes the keys and the
//! offers marked with the application's id and returns before anything
//! starts.

/// The variable `flui run --register-associations` sets in the
/// application's environment, to `1`.
#[cfg(windows)]
pub(in crate::app) const REGISTER_ENV: &str = "FLUI_REGISTER_ASSOCIATIONS";

/// The argument an uninstaller runs the executable with to remove its
/// registration.
#[cfg(any(windows, test))]
pub(in crate::app) const UNREGISTER_ARGUMENT: &str = "--flui-unregister-associations";

/// Whether a run registers what it opens: the application's own choice,
/// else a request from `flui run --register-associations`, else whether the
/// build is a release one.
#[cfg(any(windows, test))]
fn registers(chosen: Option<bool>, requested: bool, debug_build: bool) -> bool {
    chosen.unwrap_or(requested || !debug_build)
}

/// Whether `arguments`, the process's own without its name, ask for the
/// registration to be removed.
#[cfg(any(windows, test))]
fn unregister_requested(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    arguments
        .into_iter()
        .any(|argument| argument == UNREGISTER_ARGUMENT)
}

/// The registration a run makes once its main window shows its first frame.
#[cfg(windows)]
pub(in crate::app) struct PendingRegistration {
    app_id: String,
    display_name: String,
    url_schemes: &'static [&'static str],
    file_types: &'static [flui_view::navigation::FileTypeDeclaration],
}

#[cfg(windows)]
impl PendingRegistration {
    /// What a run with `settings` and `identity` registers: `None` when it
    /// declares no scheme and no file type, registers nothing in this build,
    /// runs packaged, or has no application id to mark its keys with (warned
    /// about).
    pub(in crate::app) fn of(
        settings: &super::opened::OpenSettings,
        identity: &flui_log::AppIdentity,
    ) -> Option<Self> {
        let declarations = settings.declarations?;
        let url_schemes = declarations.declared_url_schemes();
        let file_types = declarations.declared_file_types();
        if url_schemes.is_empty() && file_types.is_empty() {
            return None;
        }
        let requested = std::env::var_os(REGISTER_ENV).is_some_and(|value| value == "1");
        if !registers(
            settings.register_associations,
            requested,
            cfg!(debug_assertions),
        ) {
            tracing::debug!(
                "a development build registers its URL schemes and file types only for \
                 `flui run --register-associations` or \
                 `Application::register_associations(true)`"
            );
            return None;
        }
        if flui_native::associations::runs_packaged() {
            tracing::debug!(
                "the package manifest registers the application's URL schemes and file types"
            );
            return None;
        }
        let Some(app_id) = identity.app_id() else {
            tracing::warn!(
                "registering URL schemes and file types needs an application id \
                 (`AppConfig::application_identity`), which marks the keys as the \
                 application's: nothing is registered"
            );
            return None;
        };
        Some(Self {
            app_id: app_id.as_str().to_owned(),
            display_name: identity.display_name().to_owned(),
            url_schemes,
            file_types,
        })
    }

    /// Register on `io`, off the frame path; a pool that refuses the work
    /// (the run is ending) registers nothing, which the next run makes up.
    pub(in crate::app) fn start(self, io: &flui_runtime::execution::ExecutionServices) {
        let refused = io.spawn_io(Box::pin(async move {
            let file_types: Vec<flui_native::associations::FileType<'_>> = self
                .file_types
                .iter()
                .map(|file_type| {
                    flui_native::associations::FileType::new(
                        file_type.extension(),
                        file_type.name(),
                    )
                })
                .collect();
            let registered = flui_native::associations::register(
                &self.app_id,
                &self.display_name,
                self.url_schemes,
                &file_types,
            );
            tracing::info!(
                written = registered.written,
                current = registered.current,
                refused = registered.refused,
                removed = registered.removed,
                "registered the application's URL schemes and file types"
            );
        }));
        if let Err(error) = refused {
            tracing::debug!(%error, "the URL schemes and file types are not registered in this run");
        }
    }
}

/// Start the registration the run holds, once: the main window revealed its
/// first frame. A later reveal (a reopened main window, a secondary one)
/// finds none.
#[cfg(windows)]
pub(in crate::app) fn start_pending() {
    let taken = super::host::APP_RUNTIME.with(|slot| {
        let mut state = slot.try_borrow_mut().ok()?;
        let pending = state.pending_registration.take()?;
        Some((pending, std::sync::Arc::clone(state.ensure_execution())))
    });
    // Started with the loop state released.
    if let Some((pending, io)) = taken {
        pending.start(&io);
    }
}

/// When the process was started with `--flui-unregister-associations`:
/// delete the keys and offers registration wrote for the application of
/// `identity`, so the run ends there. `None` for any other start.
#[cfg(windows)]
pub(in crate::app) fn unregister_if_requested(
    identity: &flui_log::AppIdentity,
) -> Option<Result<(), crate::app::LaunchError>> {
    if !unregister_requested(std::env::args_os().skip(1)) {
        return None;
    }
    let Some(app_id) = identity.app_id() else {
        tracing::warn!(
            "`{UNREGISTER_ARGUMENT}` needs an application id \
             (`AppConfig::application_identity`): nothing is removed"
        );
        return Some(Ok(()));
    };
    Some(
        match flui_native::associations::unregister(app_id.as_str()) {
            Ok(removed) => {
                tracing::info!(
                    removed,
                    "removed the application's URL scheme and file type registration"
                );
                Ok(())
            }
            Err(error) => Err(crate::app::LaunchError::Platform {
                source: std::sync::Arc::new(error),
            }),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{UNREGISTER_ARGUMENT, registers, unregister_requested};

    /// The application's choice decides; without one, `flui run
    /// --register-associations` or a release build registers and a development
    /// build does not.
    #[test]
    fn a_release_build_registers_unless_the_application_says_otherwise() {
        for (chosen, requested, debug_build, expected) in [
            (None, false, false, true),
            (None, false, true, false),
            (None, true, true, true),
            (Some(false), true, false, false),
            (Some(true), false, true, true),
        ] {
            assert_eq!(
                registers(chosen, requested, debug_build),
                expected,
                "{chosen:?} {requested} {debug_build}"
            );
        }
    }

    /// The argument asks for the removal wherever it is; a link or a file in
    /// its place does not.
    #[test]
    fn the_unregister_argument_is_read_whole() {
        let arguments = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(unregister_requested(arguments(&[UNREGISTER_ARGUMENT])));
        assert!(unregister_requested(arguments(&[
            "notes://3",
            UNREGISTER_ARGUMENT
        ])));
        assert!(!unregister_requested(arguments(&[
            "--flui-unregister-associations=1"
        ])));
        assert!(!unregister_requested(arguments(&["notes://3"])));
    }
}
