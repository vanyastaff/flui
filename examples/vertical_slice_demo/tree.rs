//! The vertical-slice demo tree — shared, via `#[path]`-inclusion, between
//! `examples/vertical_slice_demo/main.rs` (mounted on a live window through
//! `flui_app::run_app`) and the root-crate acceptance test
//! `tests/vertical_slice_demo.rs` (mounted headlessly through
//! `flui_binding::HeadlessBinding`). Both consumers exercise the exact same
//! tree, so the acceptance test proves the tree the example actually runs.
//!
//! Self-contained over `flui-widgets` (plus the lower-layer `flui-view` and
//! `flui-animation` crates it already depends on for `RebuildHandle` and
//! `Curves`) — no `flui-app` import here; each consumer decides how to mount.
//!
//! Composition: a `StatefulView` root (`count`, `expanded`, `scroll_offset`)
//! builds a `Column` of:
//! - a counter row: `Text` showing the count, plus a `GestureDetector` "+"
//!   button that increments it;
//! - a fixed-height `ListView` with enough rows to overflow its box;
//! - a `GestureDetector`-wrapped `AnimatedContainer` that toggles its
//!   width/height/color between a collapsed and an expanded target.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::Curves;
use flui_view::RebuildHandle;
use flui_widgets::prelude::*;
use flui_widgets::{AnimatedContainer, column, row};

/// The fixed height of the box the list is clipped to. `pub` (along with the
/// animated box's collapsed/expanded constants below) so the acceptance test
/// can distinguish the tree's several `RenderConstrainedBox` nodes by
/// committed size instead of a duplicated magic number.
pub const LIST_BOX_HEIGHT: f32 = 200.0;
/// Per-row height of the list.
pub const LIST_ITEM_EXTENT: f32 = 32.0;
/// Row count: `LIST_ITEM_COUNT * LIST_ITEM_EXTENT` (768px) overflows
/// `LIST_BOX_HEIGHT` (200px), so the list is genuinely scrollable.
pub const LIST_ITEM_COUNT: usize = 24;

/// The animated box's width/height at rest (`expanded == false`).
pub const COLLAPSED_WIDTH: f32 = 96.0;
pub const COLLAPSED_HEIGHT: f32 = 64.0;
/// The animated box's width/height target once expanded.
pub const EXPANDED_WIDTH: f32 = 220.0;
pub const EXPANDED_HEIGHT: f32 = 140.0;
/// How long the animated box takes to reach a new target.
pub const ANIMATION_DURATION: Duration = Duration::from_millis(240);

const COLLAPSED_COLOR: Color = Color::rgb(64, 64, 90);
const EXPANDED_COLOR: Color = Color::rgb(230, 126, 34);
const PLUS_BUTTON_COLOR: Color = Color::rgb(33, 150, 243);
const BACKGROUND_COLOR: Color = Color::rgb(18, 18, 24);

/// The vertical-slice demo root.
///
/// `count`/`expanded`/`scroll_offset` are `Rc<Cell<_>>` so a caller (the
/// acceptance test) can keep a clone from before mounting: the counter and
/// the animated box are driven the same way the running example drives them
/// — by dispatching a pointer tap through the mounted `GestureDetector`s —
/// but the list here has no drag-to-scroll gesture wired yet, so its offset
/// is driven by mutating `scroll_offset` directly and forcing a rebuild
/// (documented, honest fallback; see the acceptance test).
#[derive(Clone, StatefulView)]
pub struct DemoRoot {
    /// Tap count, shown by the counter row and incremented by its "+" button.
    pub count: Rc<Cell<i32>>,
    /// Whether the animated box currently targets its expanded size/color.
    pub expanded: Rc<Cell<bool>>,
    /// The list's programmatic scroll offset, in logical pixels.
    pub scroll_offset: Rc<Cell<f32>>,
}

impl DemoRoot {
    /// A fresh demo tree at rest: `count == 0`, collapsed, unscrolled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            count: Rc::new(Cell::new(0)),
            expanded: Rc::new(Cell::new(false)),
            scroll_offset: Rc::new(Cell::new(0.0)),
        }
    }
}

impl Default for DemoRoot {
    fn default() -> Self {
        Self::new()
    }
}

/// Persistent state for [`DemoRoot`].
///
/// Captures a [`RebuildHandle`] in `init_state` (ADR-0018) so a tap callback
/// — which runs outside `build`/layout/paint — can schedule the next frame's
/// rebuild without touching the tree itself.
pub struct DemoRootState {
    count: Rc<Cell<i32>>,
    expanded: Rc<Cell<bool>>,
    scroll_offset: Rc<Cell<f32>>,
    /// `None` only before `init_state` has run; every `build` call happens
    /// after it (`ViewState` lifecycle order), so it is always `Some` there.
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for DemoRoot {
    type State = DemoRootState;

    fn create_state(&self) -> Self::State {
        DemoRootState {
            count: Rc::clone(&self.count),
            expanded: Rc::clone(&self.expanded),
            scroll_offset: Rc::clone(&self.scroll_offset),
            rebuild: None,
        }
    }
}

impl ViewState<DemoRoot> for DemoRootState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
    }

    fn build(&self, _view: &DemoRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: init_state runs before build (ViewState lifecycle order)");

        let count = self.count.get();
        let expanded = self.expanded.get();
        let scroll_offset = self.scroll_offset.get();

        // -- counter row: Text + a "+" GestureDetector button -----------------
        let count_for_tap = Rc::clone(&self.count);
        let rebuild_for_count = rebuild.clone();
        let counter_row = Row::new(row![
            Text::new(format!("Count: {count}")),
            SizedBox::width(16.0),
            GestureDetector::new()
                // Opaque: the button must be tappable across its whole
                // painted area, not only where a hit-testable descendant
                // (the "+" glyph's own ink) happens to sit.
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || {
                    count_for_tap.set(count_for_tap.get() + 1);
                    rebuild_for_count.schedule();
                })
                .child(
                    Container::new()
                        .padding(EdgeInsets::all(px(8.0)))
                        .color(PLUS_BUTTON_COLOR)
                        .child(Text::new("+")),
                ),
        ]);

        // -- fixed-height scrollable list --------------------------------------
        let list_area = SizedBox::height(LIST_BOX_HEIGHT).child(
            ListView::new(
                LIST_ITEM_EXTENT,
                (0..LIST_ITEM_COUNT)
                    .map(|index| {
                        Container::new()
                            .padding(EdgeInsets::all(px(4.0)))
                            .child(Text::new(format!("Item {index}")))
                            .boxed()
                    })
                    .collect::<Vec<_>>(),
            )
            .offset(scroll_offset),
        );

        // -- animated box: tap toggles expanded, AnimatedContainer eases to it -
        let expanded_for_tap = Rc::clone(&self.expanded);
        let rebuild_for_toggle = rebuild;
        let (target_width, target_height, target_color) = if expanded {
            (EXPANDED_WIDTH, EXPANDED_HEIGHT, EXPANDED_COLOR)
        } else {
            (COLLAPSED_WIDTH, COLLAPSED_HEIGHT, COLLAPSED_COLOR)
        };
        let animated_area = GestureDetector::new()
            // Opaque: the box's own child is a zero-size filler (the box's
            // painted color IS its content), so hit-testing must fire across
            // the whole animated box regardless of child hit-testability.
            .behavior(HitTestBehavior::Opaque)
            .on_tap(move || {
                expanded_for_tap.set(!expanded_for_tap.get());
                rebuild_for_toggle.schedule();
            })
            .child(
                // An empty `Text` filler, not `SizedBox` — a `SizedBox`
                // nested inside this container's own tight width/height
                // constraint would be tightened down to the SAME committed
                // size (Flutter's `enforce` semantics: an incoming tight
                // constraint always wins), producing a second
                // `RenderConstrainedBox` indistinguishable from the
                // container's own by geometry alone.
                AnimatedContainer::new(Text::new(""))
                    .width(target_width)
                    .height(target_height)
                    .color(target_color)
                    .duration(ANIMATION_DURATION)
                    .curve(Curves::EaseOut),
            );

        Container::new()
            .color(BACKGROUND_COLOR)
            .padding(EdgeInsets::all(px(16.0)))
            .alignment(Alignment::TOP_LEFT)
            .child(Column::new(column![counter_row, list_area, animated_area]))
    }
}

/// Build a fresh demo tree, ready to mount.
#[must_use]
pub fn demo_root() -> DemoRoot {
    DemoRoot::new()
}

/// Thin `StatelessView` entry point for [`flui_app::run_app`](https://docs.rs/flui-app),
/// which requires a stateless root — the demo's actual state lives one level
/// down, in [`DemoRoot`]. Purely a pass-through: it contributes no render
/// node and no behavior of its own, so the tree an app author sees on screen
/// is exactly [`DemoRoot`]'s. The acceptance test mounts `DemoRoot` directly
/// (skipping this adapter) to reach its `Rc<Cell<_>>` handles before mount;
/// structurally and behaviorally that is the identical tree.
#[derive(Clone, StatelessView)]
pub struct DemoApp;

impl StatelessView for DemoApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        demo_root()
    }
}
