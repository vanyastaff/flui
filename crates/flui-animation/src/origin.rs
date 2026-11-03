//! What diagnostics name a controller by: a debug label and the site that
//! created it. Both are display fields only; correlating events stays on the
//! controller span's identity (ADR-0202).

use std::borrow::Cow;

use flui_foundation::CreationSite;

/// A controller's debug label and creation site, fixed at construction.
#[derive(Clone, Debug)]
pub(crate) struct ControllerOrigin {
    pub(crate) label: Option<Cow<'static, str>>,
    pub(crate) site: CreationSite,
}
