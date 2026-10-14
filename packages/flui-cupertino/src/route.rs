//! [`cupertino_page_route`] — the iOS-style slide-in page transition, over
//! `flui-widgets`' existing [`PageRoute`] machinery.
//!
//! `PageRoute` documents itself as deliberately not extensible (no subclassing
//! in Rust, and this crate declines to expose `ModalRoute`/`TransitionRoute` as
//! bases) — so this is exactly what its own doc anticipates: a **thin
//! constructor** that configures the existing builder, not a new route type.
//!
//! ## What it configures
//!
//! - The iOS parallax slide, [`ParallaxPageTransition`], and its 500 ms
//!   duration. The slide lives in `flui-widgets` (ADR-0221 D4) so that
//!   `flui-material`'s Apple-platform default runs the same copy; its module
//!   docs record the curve wiring and the linear back-gesture tracking.
//! - A `0x18000000` barrier color, via [`PageRoute::barrier_color`] (mirroring
//!   `PopupRoute`'s existing builder method).
//! - `back_gesture(true)` by default — the iOS route wires the edge-swipe-back
//!   detector unconditionally; FLUI has no platform-theme selection, so every
//!   `cupertino_page_route` opts in (matching `flui-widgets`' own
//!   `PageRoute::back_gesture` doc, which names this exact route as the
//!   Cupertino opt-in's first real caller).
//!
//! ## Deferred, named
//!
//! - **Edge shadow.** `flui-widgets` has no `DecoratedBoxTransition` (an
//!   `Animation<Decoration>`-driven proxy) and `flui-painting`'s `BoxDecoration`
//!   has no lerp-equivalent trait for tweening an arbitrary decoration; adding
//!   either is out of this change's scope. The primary page's leading edge
//!   paints with no shadow gradient during the transition.
//! - **`title`/`previous_title`.** These exist to auto-populate
//!   `CupertinoNavigationBar`'s `middle`/large title when the app author does
//!   not supply one — a consumer this crate does not ship yet. Deferred with
//!   that consumer.
//! - **`fullscreen_dialog`** (the bottom-up sheet transition, and the
//!   barrier/back-gesture/edge-shadow suppression that comes with it). Not
//!   modeled — `flui-widgets`' `PageRoute` has no `fullscreen_dialog` flag yet
//!   (`back_gesture.rs`'s module doc already records this same gap for the
//!   detector).
//! - **A delegated transition** (letting a covered route borrow the
//!   *covering* route's own transition instead of running its own secondary
//!   animation). FLUI has no declarative-Navigator API to delegate through.
//! - **`can_transition_to`/`can_transition_from`.** Secondary-animation
//!   coordination could be narrowed to routes that are *specifically*
//!   Cupertino. `flui-widgets`' existing (private) `TransitionGroup::Page`
//!   coordinates every `PageRoute` together, Cupertino-styled or not — broader
//!   than that.
//! - A modal popup route and a dialog route — separate route types, out of
//!   this component's scope.

use std::rc::Rc;

use flui_sdk::painting::Color;
use flui_sdk::view::BoxedView;
use flui_sdk::view::prelude::BuildContext;
use flui_sdk::widgets::{PageRoute, ParallaxPageTransition, RouteAnimation};

/// The barrier dim painted during the transition, `0x18000000` — an eyeballed
/// estimate of the iOS dim.
fn barrier_color() -> Color {
    Color::from_argb(0x1800_0000)
}

/// A `cupertino_page_route`-configured [`PageRoute`], showing `builder` with
/// the iOS slide transition, a transition-only barrier dim, and an
/// edge-swipe-back gesture — see the module docs for exactly what is and is
/// not supported.
///
/// Returns a plain [`PageRoute<T>`], not a distinct wrapper type: every other
/// `PageRoute` builder method (`.named(...)`, `.maintain_state(...)`, …) stays
/// available to chain afterward, exactly as if this were `PageRoute::new`
/// itself with Cupertino's defaults pre-applied.
///
/// ```
/// use flui_cupertino::cupertino_page_route;
/// use flui_sdk::widgets::Text;
/// use flui_sdk::view::prelude::*;
///
/// let route = cupertino_page_route::<(), _>(|_ctx, _primary, _secondary| {
///     Text::new("Details").into_view().boxed()
/// })
/// .named("/details");
/// # let _ = route;
/// ```
#[must_use]
pub fn cupertino_page_route<T, F>(builder: F) -> PageRoute<T>
where
    T: Send + Clone + 'static,
    F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation) -> BoxedView + 'static,
{
    PageRoute::new(builder)
        .transition_builder(Rc::new(ParallaxPageTransition::new()))
        .barrier_color(barrier_color())
        .back_gesture(true)
}
