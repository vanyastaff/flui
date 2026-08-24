//! A component whose built child changes TYPE across a rebuild replaces a
//! render object rather than updating one — and the replacement must be laid
//! out in the same frame it is mounted in.
//!
//! This used to be covered only incidentally, through `Image`: its async
//! dispatch wrapped the leaf in a combinator while a load was in flight and
//! built a bare leaf once the decode was cached, so a provider swap changed
//! the child's type. That reproducer found a real failure — the replacement
//! mounted but never received a layout pass, so a geometry query panicked
//! with "render node should have box geometry after layout". `Image` no
//! longer changes its child's type, which is exactly why the scenario is
//! written here directly: coverage that survives only as a side effect of an
//! unrelated widget's internals is coverage that disappears without notice.
//!
//! Both positions matter and neither subsumes the other:
//!
//! - At the **pipeline root**, the replacement also re-roots the pipeline —
//!   the harness has to re-establish `root_id` and the root constraints for
//!   an identity that changed under it.
//! - **Below the root**, the parent sizes itself from the child it just laid
//!   out, so a child left without committed geometry cannot produce the
//!   expected parent size. A root-only test could not tell a real fix from
//!   the scenario quietly ceasing to replace the root render object.

use crate::common::{lay_out, loose, size};
use flui_widgets::prelude::{BuildContext, IntoView, StatelessView};
use flui_widgets::{Padding, SizedBox};

/// Builds either a wrapped box (`Padding` over `SizedBox` — two render
/// objects) or a bare one (`SizedBox` alone), so a rebuild that flips
/// `wrapped` changes the built child's concrete type. Both forms shrink-wrap,
/// so the assertion is on a size the swap actually changes.
#[derive(Clone, Debug, StatelessView)]
struct Swapper {
    wrapped: bool,
}

impl StatelessView for Swapper {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_view::ViewExt;
        if self.wrapped {
            Padding::all(3.0).child(SizedBox::square(10.0)).boxed()
        } else {
            SizedBox::square(20.0).boxed()
        }
    }
}

#[test]
fn a_replaced_root_render_object_is_laid_out_in_the_frame_it_is_mounted() {
    let mut laid = lay_out(Swapper { wrapped: true }, loose(1000.0));
    assert_eq!(
        laid.size(laid.current_root()),
        size(16.0, 16.0),
        "the wrapped form is a 10px box inside 3px of padding",
    );

    laid.pump_widget(Swapper { wrapped: false });

    assert_eq!(
        laid.size(laid.current_root()),
        size(20.0, 20.0),
        "the bare form's render object replaced the root, and must carry \
         committed geometry on the frame of the swap -- not be mounted \
         without ever being laid out",
    );
}

#[test]
fn a_replaced_render_object_below_the_root_is_laid_out_in_the_frame_it_is_mounted() {
    let mut laid = lay_out(
        Padding::all(2.0).child(Swapper { wrapped: true }),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.current_root()), size(20.0, 20.0));

    laid.pump_widget(Padding::all(2.0).child(Swapper { wrapped: false }));

    // 20x20 child + 2px padding on every side. A child left without committed
    // geometry cannot produce this, because the padding parent sizes itself
    // from the child it just laid out.
    assert_eq!(
        laid.size(laid.current_root()),
        size(24.0, 24.0),
        "a replaced render object below the root must be laid out in the same \
         frame as the swap that created it",
    );
}
