//! Autostart over the `Background` portal.
//!
//! The portal is the one path inside Flatpak (and the one a desktop
//! sanctions outside it): `RequestBackground` with `autostart` writes or
//! removes the application's entry in `~/.config/autostart`, asking the user
//! where the desktop's policy says so. Outside a sandbox the portal knows
//! the application only by the id it was named by ([`super::Bus::desktop`]);
//! with none it refuses (`Response` 2), which answers
//! [`SystemFailure::PortalResponse`]. Snap has no autostart through the
//! portal, which refuses it the same way. No file is written by FLUI itself:
//! a second writer of the autostart entry would disagree with the portal's
//! record of what the user allowed.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use flui_platform::capability::{CompletionSource, ProviderContext, Resolver, WindowRef};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use zbus::{OwnedVariant, Variant};

use super::request::{self, Response};
use super::{Interface, Portal, PortalError, with_parent};
use crate::autostart::{
    AutostartBackend, AutostartCapability, AutostartError, AutostartRequest, AutostartResult,
    AutostartState,
};

const BACKGROUND: Interface = Interface::new("org.freedesktop.portal.Background", 1);

/// The longest first item of a command line the portal takes, in
/// characters.
const MAX_PROGRAM_CHARS: usize = 256;

/// [`AutostartBackend`] over the desktop portal.
pub(crate) struct PortalAutostart {
    portal: Rc<Portal>,
    /// The window the portal's question is attached to.
    window: Option<WindowRef>,
    completions: CompletionSource,
}

impl PortalAutostart {
    pub(crate) fn new(portal: Rc<Portal>, ctx: &ProviderContext<'_>) -> Self {
        Self {
            portal,
            window: ctx.window(),
            completions: ctx.completions().clone(),
        }
    }
}

impl AutostartBackend for PortalAutostart {
    fn request(&self, request: AutostartRequest, answer: Resolver<AutostartResult>) {
        if let Err(error) = self.portal.start() {
            let _ = answer.resolve(Err(autostart_error(error)));
            return;
        }
        let portal = Rc::clone(&self.portal);
        with_parent(self.window.as_ref(), &self.completions, move |parent| {
            let token = portal.token();
            portal.spawn(move |bus| async move {
                let options = match options(&request, &token) {
                    Ok(options) => options,
                    Err(refusal) => {
                        let _ = answer.resolve(Err(refusal));
                        return;
                    }
                };
                let body = (parent.as_str().to_owned(), options);
                let result = async {
                    let desktop = bus.desktop().await?;
                    request::call(&desktop, BACKGROUND, "RequestBackground", &token, &body, None)
                        .await
                }
                .await;
                // The window's export lasts until the request ended.
                drop(parent);
                let _ = answer.resolve(match result {
                    Ok(Response::Answered(results)) => granted(&results),
                    Ok(Response::Cancelled) => Err(AutostartError::Denied),
                    Ok(Response::Ended(code)) => Err(SystemFailure::PortalResponse(code).into()),
                    // The UI runtime retired: nobody hears the answer.
                    Ok(Response::Dismissed) => return,
                    Err(error) => Err(autostart_error(error)),
                });
            });
        });
    }
}

/// The options of one request: its token, whether to start at login, the
/// reason, and the program to start, which this process is.
///
/// Read on the portal's thread (`/proc/self/exe`). The portal joins the
/// command line with spaces into the entry's `Exec` without quoting it, so
/// a program whose path a desktop entry cannot hold unquoted is refused here
/// rather than written as an entry that starts something else.
fn options(
    request: &AutostartRequest,
    token: &str,
) -> Result<HashMap<&'static str, Variant<'static>>, AutostartError> {
    let mut options = HashMap::from([
        ("handle_token", Variant::from(token.to_owned())),
        ("autostart", Variant::from(request.is_enabled())),
    ]);
    if let Some(reason) = &request.reason {
        options.insert("reason", Variant::from(reason.clone()));
    }
    if request.is_enabled() {
        let program = std::env::current_exe()
            .map_err(|error| SystemFailure::Os(error.raw_os_error().unwrap_or(libc::EIO)))?;
        let program = entry_program(&program).ok_or(Unsupported::of::<AutostartCapability>(
            UnsupportedReason::NotOnThisDevice,
        ))?;
        options.insert("commandline", Variant::from(vec![program]));
    }
    Ok(options)
}

/// `program` as a desktop entry's `Exec` holds it unquoted: an absolute UTF-8
/// path of at most [`MAX_PROGRAM_CHARS`] characters with no whitespace, no
/// quote, no `\`, `$`, `` ` `` or `%` (a field code), and no control
/// character; `None` otherwise.
fn entry_program(program: &Path) -> Option<String> {
    let program = program.to_str()?;
    let plain = program.starts_with('/')
        && program.chars().count() <= MAX_PROGRAM_CHARS
        && !program.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '"' | '\'' | '\\' | '$' | '`' | '%')
        });
    plain.then(|| program.to_owned())
}

/// What the portal granted: refused when it did not let the application run
/// in the background, else whether it starts it at login.
fn granted(results: &HashMap<String, OwnedVariant>) -> AutostartResult {
    let flag = |name: &str| {
        results
            .get(name)
            .map(|value| bool::try_from(value.clone()))
            .transpose()
            .map_err(|_| SystemFailure::MalformedAnswer)
    };
    if flag("background")? == Some(false) {
        return Err(AutostartError::Denied);
    }
    Ok(if flag("autostart")? == Some(true) {
        AutostartState::Enabled
    } else {
        AutostartState::Disabled
    })
}

fn autostart_error(error: PortalError) -> AutostartError {
    match error {
        PortalError::Unsupported(unsupported) => AutostartError::Unsupported(unsupported),
        PortalError::Failed(failure) => AutostartError::Failed(failure),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::entry_program;

    #[test]
    fn only_a_program_an_entry_holds_unquoted_is_started_at_login() {
        let cases = [
            ("/usr/bin/notes", true),
            ("/app/bin/notes", true),
            ("relative/notes", false),
            ("/opt/My Notes/notes", false),
            ("/opt/notes%u", false),
            ("/opt/$HOME/notes", false),
            ("/opt/\"notes\"", false),
        ];
        for (program, plain) in cases {
            assert_eq!(
                entry_program(Path::new(program)).is_some(),
                plain,
                "{program}"
            );
        }
        let long = format!("/{}", "a".repeat(super::MAX_PROGRAM_CHARS));
        assert!(entry_program(Path::new(&long)).is_none());
    }
}
