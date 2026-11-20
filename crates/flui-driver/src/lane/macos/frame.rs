//! What the window frame adds to an app's accessibility tree on macOS, taken out so the tree reads
//! as the app published it (ADR-0095 §4).
//!
//! The `NSWindow` is the window's own accessibility element (`AXWindow`), and its children are
//! the frame's controls (the close, minimize, zoom and full-screen buttons, by their subroles)
//! beside the content view's. A FLUI content view publishes the app's root node, the window
//! itself, which the adapter reports as an untitled `AXGroup` with no subrole and no identifier:
//! the content's nodes are that group's children. [`project`] keeps the `AXWindow` as the tree's
//! root, drops the frame's controls, and lifts that one group's children into the root, so the
//! root's children are the app's top-level nodes, as they are in the app's own tree and in UI
//! Automation's.

use flui_protocol::tree::Node;

/// The subroles of the frame's own controls.
pub const FRAME_SUBROLES: &[&str] = &[
    "AXCloseButton",
    "AXMinimizeButton",
    "AXZoomButton",
    "AXFullScreenButton",
];

/// Whether `node`, a child of the window's element, is one of the frame's own controls, by the
/// subrole its native role carries (`roles::native_role`).
#[must_use]
pub fn is_frame_control(node: &Node) -> bool {
    node.native_role
        .split_once('/')
        .is_some_and(|(_, subrole)| FRAME_SUBROLES.contains(&subrole))
}

/// Whether `node` is the group the adapter reports for the app's root node: an `AXGroup` with no
/// subrole, no name and no identifier.
fn is_content_root(node: &Node) -> bool {
    node.native_role == "AXGroup"
        && node.name.is_none()
        && node.automation_id.is_none()
        && node.identifier.is_none()
}

/// `window`, the window's element read with everything under it, as the app published it: see
/// the [module docs](self). A window whose content is not one such group keeps its children as
/// they are, the frame's controls left out.
#[must_use]
pub fn project(mut window: Node) -> Node {
    window.children.retain(|child| !is_frame_control(child));
    if let [content] = window.children.as_slice()
        && is_content_root(content)
    {
        let content = window.children.remove(0);
        window.children_unread |= content.children_unread;
        window.children = content.children;
    }
    window
}
