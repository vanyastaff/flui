//! [`resolve_alignment`] against a mounted [`Directionality`]. The inherited
//! view's own predicates and the axis-direction helpers stay unit tests in
//! `src/localization/directionality.rs`.

use std::cell::Cell;
use std::rc::Rc;

use flui_types::Alignment;
use flui_types::typography::TextDirection;
use flui_view::prelude::*;
use flui_widgets::SizedBox;
use flui_widgets::localization::{Directionality, resolve_alignment};

use crate::common::harness::mount;

/// A directional alignment resolves to opposite edges under the two
/// directions, through a real mounted `Directionality`.
///
/// The whole point of the seam: `AlignmentGeometry::resolve(is_ltr)` was
/// already correct and already tested, and the parity corpus called it at
/// the call site with a literal `false` because "no widget-surface path
/// reads one" (`tests/parity/align_test.rs`). This is that path.
#[test]
fn a_directional_alignment_resolves_against_a_mounted_directionality() {
    use flui_types::layout::AlignmentDirectional;

    #[derive(Clone, StatelessView)]
    struct Probe {
        seen: Rc<Cell<Option<Alignment>>>,
    }

    impl StatelessView for Probe {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            self.seen.set(Some(resolve_alignment(
                ctx,
                AlignmentDirectional::new(-1.0, 0.0),
            )));
            SizedBox::shrink()
        }
    }

    for (direction, expected_x) in [(TextDirection::Ltr, -1.0), (TextDirection::Rtl, 1.0)] {
        let seen = Rc::new(Cell::new(None));
        let _harness = mount(Directionality::new(
            direction,
            Probe {
                seen: Rc::clone(&seen),
            },
        ));
        let resolved = seen.get().expect("the probe must have built");
        assert!(
            (resolved.x - expected_x).abs() < f32::EPSILON,
            "start is the {direction:?} reading edge, so x must be \
             {expected_x}, got {}",
            resolved.x
        );
    }
}
