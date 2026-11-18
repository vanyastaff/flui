//! How a desktop portal names the window a dialog it shows belongs to.

use std::{any::Any, fmt, num::NonZeroU32};

/// A window as an XDG desktop portal names the parent of a dialog or chooser
/// it shows for the application (XDG Desktop Portal, "Window identifiers"):
/// `x11:<XID in hex>` for an X11 window, `wayland:<handle>` for a Wayland
/// window the compositor exported (`xdg-foreign`), and the empty string for
/// no window, which the portal shows unattached.
///
/// It holds what keeps the name valid: a Wayland window's export lives until
/// this value drops, so a request keeps its parent until the request ends, and
/// the handle names nothing once it did. An X11 id needs nothing held.
///
/// A window answers it through
/// [`PlatformWindow::portal_parent`](crate::PlatformWindow::portal_parent),
/// and a provider asks it through
/// [`WindowRef::portal_parent`](super::WindowRef::portal_parent). `Send`: a
/// request that ends on the portal's own thread drops it there.
pub struct PortalParent {
    identifier: String,
    /// What the name lasts by: dropped with the value.
    export: Option<Box<dyn Any + Send>>,
}

impl PortalParent {
    /// No window: a dialog the portal shows unattached.
    #[must_use]
    pub fn none() -> Self {
        Self {
            identifier: String::new(),
            export: None,
        }
    }

    /// The X11 window `xid`.
    #[must_use]
    pub fn x11(xid: NonZeroU32) -> Self {
        Self {
            identifier: format!("x11:{xid:x}"),
            export: None,
        }
    }

    /// A Wayland window the compositor exported under `handle`, named while
    /// `export` lives: its drop releases the export (`zxdg_exported_v2.destroy`),
    /// after which the handle names nothing.
    ///
    /// The handle is the compositor's string as `xdg-foreign` hands it out; a
    /// Wayland string holds no NUL byte.
    #[must_use]
    pub fn wayland(handle: &str, export: impl Any + Send) -> Self {
        Self {
            identifier: format!("wayland:{handle}"),
            export: Some(Box::new(export)),
        }
    }

    /// The `parent_window` argument a portal call takes.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.identifier
    }

    /// Whether it names no window.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.identifier.is_empty()
    }
}

impl Default for PortalParent {
    fn default() -> Self {
        Self::none()
    }
}

impl fmt::Debug for PortalParent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortalParent")
            .field("identifier", &self.identifier)
            .field("holds_export", &self.export.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::PortalParent;

    struct Released(Arc<AtomicBool>);

    impl Drop for Released {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_portal_names_each_kind_of_window_and_a_wayland_name_lasts_with_its_export() {
        let xid = NonZeroU32::new(0x2a).expect("an id");
        assert_eq!(PortalParent::x11(xid).as_str(), "x11:2a");
        assert_eq!(PortalParent::none().as_str(), "");
        assert!(PortalParent::none().is_none());

        let released = Arc::new(AtomicBool::new(false));
        let parent = PortalParent::wayland("a1b2-c3", Released(Arc::clone(&released)));
        assert_eq!(parent.as_str(), "wayland:a1b2-c3");
        assert!(!released.load(Ordering::SeqCst), "held while the name is");
        drop(parent);
        assert!(released.load(Ordering::SeqCst), "released with the name");
    }
}
