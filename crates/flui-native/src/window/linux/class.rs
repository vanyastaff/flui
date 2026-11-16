//! The name a Linux desktop matches a window to its application by.
//!
//! The shell finds a window's desktop entry, and with it the name, the icon
//! and the launcher the window groups under, by the Wayland toplevel's app id
//! (`xdg_toplevel.set_app_id`) or the X11 window's `WM_CLASS`, held against
//! the entry's file name and its `StartupWMClass`. `flui build linux` names
//! the entry `<application id>.desktop` and writes the identifier as its
//! `StartupWMClass`, so both hosts carry the application id the host was
//! started with ([`AppId`]), the same on every window, decided here once per
//! host. An application started without one carries its executable's file
//! stem in ASCII (`flui_foundation::executable_class`), which names its desktop
//! entry then, and claims no application id (`_GTK_APPLICATION_ID`).

#![forbid(unsafe_code)]

use std::path::Path;

use flui_foundation::{AppId, executable_class};

/// The class every window of a Linux host carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppClass {
    /// The application id the host was started with.
    Application(AppId),
    /// The executable's file stem in ASCII, for an application started
    /// without an id (`flui` when the executable cannot be named).
    Executable(Box<str>),
}

impl AppClass {
    /// The class of a host started with `app_id`, whose process runs the
    /// executable at `executable`.
    pub(crate) fn of(app_id: Option<&AppId>, executable: Option<&Path>) -> Self {
        if let Some(app_id) = app_id {
            return Self::Application(app_id.clone());
        }
        let stem = executable
            .and_then(Path::file_stem)
            .map_or_default(|stem| stem.to_string_lossy());
        Self::Executable(executable_class(&stem).into_boxed_str())
    }

    /// The class of a host started with `app_id` in this process.
    #[cfg(linux_host)]
    pub(crate) fn current(app_id: Option<&AppId>) -> Self {
        Self::of(app_id, std::env::current_exe().ok().as_deref())
    }

    /// The Wayland app id, and both halves of the X11 `WM_CLASS`.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Application(app_id) => app_id.as_str(),
            Self::Executable(stem) => stem,
        }
    }

    /// The application id the window claims as its own
    /// (`_GTK_APPLICATION_ID`), which only a declared id is.
    #[cfg(any(x11_host, test))]
    pub(crate) fn application_id(&self) -> Option<&str> {
        match self {
            Self::Application(app_id) => Some(app_id.as_str()),
            Self::Executable(_) => None,
        }
    }

    /// The X11 `WM_CLASS` value: the instance name, then the class name,
    /// each ended by a zero (ICCCM §4.1.2.5). Both are [`name`](Self::name),
    /// so a `StartupWMClass` matches whichever of the two a shell reads.
    #[cfg(any(x11_host, test))]
    pub(crate) fn wm_class(&self) -> Vec<u8> {
        let name = self.name().as_bytes();
        let mut value = Vec::with_capacity(2 * name.len() + 2);
        for _ in 0..2 {
            value.extend_from_slice(name);
            value.push(0);
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use flui_foundation::AppId;

    use super::AppClass;

    /// A window carries the application id it was started with, as its name,
    /// its claimed id and both halves of `WM_CLASS`; without one, the
    /// executable's stem in ASCII, which claims no id.
    #[test]
    fn a_window_is_classed_by_the_application_id_or_the_executable() {
        let notes = AppId::try_new("com.example.Notes").expect("an application id");
        let cases = [
            (
                Some(&notes),
                Some(Path::new("/usr/bin/notes")),
                "com.example.Notes",
                Some("com.example.Notes"),
            ),
            (
                Some(&notes),
                None,
                "com.example.Notes",
                Some("com.example.Notes"),
            ),
            (
                None,
                Some(Path::new("/opt/notes/notes-app.bin")),
                "notes-app",
                None,
            ),
            (None, Some(Path::new("/opt/nötes")), "n_tes", None),
            (None::<&AppId>, None::<&Path>, "flui", None::<&str>),
        ];
        for (app_id, executable, name, application_id) in cases {
            let class = AppClass::of(app_id, executable);
            assert_eq!(class.name(), name, "{app_id:?} {executable:?}");
            assert_eq!(class.application_id(), application_id, "{name}");
            assert_eq!(
                class.wm_class(),
                format!("{name}\0{name}\0").into_bytes(),
                "{name}"
            );
        }
    }
}
