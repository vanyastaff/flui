//! Unit tests of the private [`ModalRoute`]'s export boundary and handle. The
//! mounted wiring suite lives in `crates/flui-widgets/tests/modal_route.rs`.

use std::rc::Rc;
use std::time::Duration;

use flui_view::prelude::*;

use super::modal_route::{ModalHandle, ModalRoute};
use crate::SizedBox;

const FRAME: Duration = Duration::from_millis(300);

/// `ModalRoute` and `ModalHandle` stay private: they are the
/// implementation `PageRoute` / `PopupRoute` are built on, and exporting them as
/// extensible bases is a separate sign-off.
///
/// Red-check: add `pub use modal_route::ModalRoute;` to `navigator/mod.rs`.
#[test]
fn modal_route_is_not_exported() {
    super::export_guard::assert_not_exported(
        "lib.rs",
        include_str!("../lib.rs"),
        &["ModalRoute", "ModalHandle", "ModalScope"],
    );
    super::export_guard::assert_not_exported(
        "navigator/mod.rs",
        include_str!("mod.rs"),
        &["ModalRoute", "ModalHandle", "ModalScope"],
    );
}

/// [`ModalHandle`] is an owned capability: every clone names the
/// same route, so a handle taken before `push_bound` still drives it afterwards.
#[test]
fn modal_handle_is_cloneable_and_shares_state() {
    let route: ModalRoute<i32> = ModalRoute::new(
        FRAME,
        Rc::new(|_ctx: &dyn BuildContext, _a: &_, _s: &_| {
            SizedBox::new(10.0, 10.0).into_view().boxed()
        }),
    );
    let a = route.handle();
    let b: ModalHandle = a.clone();

    assert!(!a.offstage());
    // Unpushed: no binding, so `changed_internal_state` is inert; the flag flips
    // anyway, which is what makes a pre-push `set_offstage` legal.
    b.set_offstage(true);
    assert!(a.offstage(), "both handles name the same route");
}
