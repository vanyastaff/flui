//! Framework seams shared by the `flui-*` widget crates.
//!
//! **Only for crates in this workspace (`crates/flui-*`). No semver
//! guarantees**: anything here may change or disappear in any release.
//! Applications, examples and third-party crates must not import it.
//!
//! Each item is widget-crate plumbing that a sibling crate (scrolling,
//! navigation, text editing) needs from this one but that is not author API:
//! focus-tree wiring, the render-id anchor, a repaint-boundary keying
//! option, the impl macro for generic multi-child render views, and the
//! state every text form field shares whichever input it draws.

pub use crate::__generic_render_view_element as generic_render_view_element;
pub use crate::anchored_box::AnchoredBox;
pub use crate::form::text_form_field_core::{
    TextFormFieldConfig, TextFormFieldCore, TextFormFieldInput,
};
pub use crate::interaction::focus::{enclosing_focus_parent, install_rect_provider};
pub use crate::paint::repaint_boundary::SaltingChildKey;
