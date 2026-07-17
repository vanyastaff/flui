//! The Material sample-app tree — shared, via `#[path]`-inclusion, between
//! `examples/material_demo/main.rs` (mounted on a live window through
//! `flui_app::run_app`) and the root-crate acceptance test
//! `tests/material_demo.rs` (mounted headlessly through
//! `flui_binding::HeadlessBinding`). Both consumers exercise the exact same
//! tree, so the acceptance test proves the tree the example actually runs.
//!
//! Built entirely on `flui-material`'s and `flui-widgets`' public APIs — no
//! raw render objects. This is the Catalog.1 Material sample-app exit
//! criterion: "A Material sample app (`Scaffold` + `AppBar` +
//! `FloatingActionButton` + a `ListView` of `Card`s + a `Dialog`) renders and
//! is interactive."
//!
//! # Composition
//!
//! [`MaterialDemoRoot`] is the `Navigator` shell (same split as
//! `examples/vertical_slice_demo/tree.rs`'s `DemoRoot`/`DemoHome`): its
//! [`MaterialDemoRootState`] owns a `NavigatorHandle` and seeds it, once, with
//! a home route whose content is [`MaterialDemoHome`] — wrapped once, at the
//! very root, in `Theme(ThemeData::light())` ([`MaterialDemoApp`]).
//!
//! [`MaterialDemoRootState`] wraps its `Navigator` in a [`ScaffoldMessenger`]
//! (the scope-mount pattern — see that type's own module docs) so every
//! route's `Scaffold` shares one snack-bar queue for the whole app's life,
//! the way a real `MaterialApp` mounts one at the root.
//!
//! [`MaterialDemoHome`] builds a `Scaffold`:
//! - `app_bar`: an `AppBar` titled [`APP_TITLE`] with one action — an
//!   `IconButton` (a settings glyph) that pushes [`settings_route`]. Its
//!   `AppBar` has no explicit `leading`, so on the settings route (a second
//!   stack entry, `can_pop() == true`) it synthesizes a `BackButton` — proven
//!   by [`tests`](../../tests/material_demo.rs), not merely asserted.
//! - `body`: a selected-item `Text`, then a drag-to-scroll `ListView` of
//!   `Card`s (each `Card(InkWell(Padding(Text)))`, the canonical Material
//!   tap-target composition). Tapping a card sets the selected-item text.
//!   The drag wiring mirrors `vertical_slice_demo`'s own list: a plain
//!   `GestureDetector` feeding a `ScrollController` directly, not
//!   `Scrollable` (see that module's doc for why `Scrollable` can't host an
//!   arbitrary scrollable child yet).
//! - `floating_action_button`: a `FloatingActionButton` labeled "+" that
//!   calls [`show_dialog`] with an `AlertDialog` ("Add item"): `Cancel`
//!   (`TextButton`) pops with no change, `Add` (`FilledButton`) appends a
//!   fresh item, pops, and shows [`SNACK_BAR_ADDED_MESSAGE`] via the
//!   ambient `ScaffoldMessenger` — auto-dismissing after its own default
//!   4s duration. `show_dialog` pushes a `PopupRoute` (`opaque: false`,
//!   `maintain_state: true`), so the home route stays mounted, laid out,
//!   and painted beneath the dialog's barrier — only its hit-testing is
//!   blocked (the full-screen barrier sits on top).
//!
//! # Honest caveats (Catalog.1 exit criterion, Material half)
//!
//! This app proves the six named components mount, lay out, and respond to
//! real gesture dispatch. It does **not** exercise:
//! - **Ink ripple/splash visuals** — `InkWell` here paints only the static
//!   resolved overlay fill (see `flui_material::ink_well`'s module docs); no
//!   ripple animation exists in this substrate yet.
//! - **Component themes** (`AppBarTheme`, `CardTheme`, …) — every widget
//!   resolves the fixed M3 baseline token tables; `ThemeData` carries no
//!   component-theme overrides yet.
//! - **`Drawer`** — no `Scaffold::drawer` is configured in this demo tree
//!   (`crates/flui-material/tests/drawer.rs` covers that widget directly).
//! - **`SnackBar` action/multi-scaffold fan-out** — the "Item added"
//!   snack bar carries no action button, and this demo has only one
//!   `Scaffold` registered with its `ScaffoldMessenger` (both covered
//!   directly by `crates/flui-material/tests/snack_bar.rs`).
//!
//! The Cupertino half of the Catalog.1 exit criterion is untouched by this
//! app.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_material::{
    AlertDialog, AppBar, Card, FilledButton, FloatingActionButton, IconButton, InkWell, Scaffold,
    ScaffoldMessenger, ScaffoldMessengerHandle, ScaffoldMessengerScope, SnackBar, TextButton,
    Theme, ThemeData, show_dialog,
};
use flui_view::RebuildHandle;
use flui_widgets::column;
use flui_widgets::prelude::*;

/// How many cards the list starts with — enough to overflow any reasonably
/// sized window, so the scroll acceptance test exercises a real overflow.
pub const INITIAL_ITEM_COUNT: usize = 20;
/// Prefix shared by every item's label (`"{ITEM_LABEL_PREFIX}{n}"`), both the
/// initial 20 and any later appended by the dialog's "Add" action.
pub const ITEM_LABEL_PREFIX: &str = "Item ";
/// Fixed per-card row height — the list rides `ListView::new` (a
/// `SliverFixedExtentList`), matching `vertical_slice_demo`'s own static-list
/// choice.
pub const ITEM_EXTENT: f32 = 72.0;

/// The app bar's title.
pub const APP_TITLE: &str = "Material Demo";
/// The settings route's app bar title — distinct from every home-route text
/// so tests can tell the two routes apart by rendered content alone.
pub const SETTINGS_ROUTE_TITLE: &str = "Settings";
/// The settings route's body text.
pub const SETTINGS_ROUTE_TEXT: &str = "Settings route";

/// The FAB's child label.
pub const FAB_LABEL: &str = "+";
/// The "Add item" dialog's title.
pub const ADD_DIALOG_TITLE: &str = "Add item";
/// The "Add item" dialog's content text.
pub const ADD_DIALOG_CONTENT: &str = "A new item will be appended to the list.";
/// The dialog's dismissive action label.
pub const CANCEL_LABEL: &str = "Cancel";
/// The dialog's confirming action label.
pub const ADD_LABEL: &str = "Add";

/// The [`SnackBar`] message shown once "Add" appends an item.
pub const SNACK_BAR_ADDED_MESSAGE: &str = "Item added";

/// `Icons.settings`'s codepoint (`icons.dart`'s `settings` constant) — the
/// app bar action's glyph. Renders as tofu (no bundled icon font; see
/// `flui_widgets::Icon`'s module docs), same named gap
/// `flui_material::back_button` already carries for its own arrow glyph.
///
/// `pub` so the acceptance test can compute the identical
/// [`IconData::code_point_string`] to locate this button's rendered glyph in
/// the mounted tree, the same way it locates `BackButton`'s via
/// `flui_material::back_button::back_arrow_icon_data`.
#[must_use]
pub fn settings_icon_data() -> IconData {
    IconData::new(0xE8B8).with_font_family("MaterialIcons")
}

/// The Material demo root: a `Navigator` shell over the home route.
///
/// `items`/`selected`/`home_create_count` are `Rc`-shared so a caller (the
/// acceptance test) can keep a clone from before mounting — the same pattern
/// `vertical_slice_demo::DemoRoot` uses, for the same reason (see
/// [`home_create_count`](Self::home_create_count)'s field doc).
#[derive(Clone, StatefulView)]
pub struct MaterialDemoRoot {
    /// The list's items, in display order. Seeded with [`INITIAL_ITEM_COUNT`]
    /// entries; the "Add item" dialog's `Add` action appends to it.
    pub items: Rc<RefCell<Vec<String>>>,
    /// The most recently tapped card's label, or `None` before any tap.
    pub selected: Rc<RefCell<Option<String>>>,
    /// How many times [`MaterialDemoHomeState::create_state`] has run — a
    /// discriminator, not app-visible data. `items`/`selected` are
    /// `Rc<RefCell<_>>`s shared with the seed closure below, so they read
    /// back correctly whether `MaterialDemoHomeState` survives a dialog
    /// round trip or is torn down and rebuilt from the same closure-captured
    /// cells — a display assertion on them alone cannot tell those two cases
    /// apart. This counter can, because `create_state` runs once per element
    /// lifetime — the acceptance test reads it, not the running app.
    pub home_create_count: Rc<Cell<u32>>,
}

impl MaterialDemoRoot {
    /// A fresh demo tree: [`INITIAL_ITEM_COUNT`] items, nothing selected.
    #[must_use]
    pub fn new() -> Self {
        let items = (0..INITIAL_ITEM_COUNT)
            .map(|index| format!("{ITEM_LABEL_PREFIX}{index}"))
            .collect();
        Self {
            items: Rc::new(RefCell::new(items)),
            selected: Rc::new(RefCell::new(None)),
            home_create_count: Rc::new(Cell::new(0)),
        }
    }
}

impl Default for MaterialDemoRoot {
    fn default() -> Self {
        Self::new()
    }
}

/// Persistent state for [`MaterialDemoRoot`]: owns the `NavigatorHandle` and
/// seeds its one home route, once, in `create_state` — see
/// `vertical_slice_demo::DemoRootState`'s doc for why re-seeding on every
/// `build` would be wrong.
pub struct MaterialDemoRootState {
    navigator: NavigatorHandle,
}

impl StatefulView for MaterialDemoRoot {
    type State = MaterialDemoRootState;

    fn create_state(&self) -> Self::State {
        let navigator = NavigatorHandle::new();
        let items = Rc::clone(&self.items);
        let selected = Rc::clone(&self.selected);
        let home_create_count = Rc::clone(&self.home_create_count);
        let navigator_for_home = navigator.clone();
        navigator.seed_initial(
            SimpleRoute::<()>::new(move |_ctx| {
                MaterialDemoHome {
                    items: Rc::clone(&items),
                    selected: Rc::clone(&selected),
                    navigator: navigator_for_home.clone(),
                    create_count: Rc::clone(&home_create_count),
                }
                .into_view()
                .boxed()
            })
            .named("/"),
        );
        MaterialDemoRootState { navigator }
    }
}

impl ViewState<MaterialDemoRoot> for MaterialDemoRootState {
    fn build(&self, _view: &MaterialDemoRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        // The scope-mount pattern: one `ScaffoldMessenger` above the whole
        // `Navigator`, so every route's `Scaffold` shares one snack-bar
        // queue for the app's life — see the module docs.
        ScaffoldMessenger::new(Navigator::new(self.navigator.clone()))
    }
}

/// The home route's content: the `Scaffold` with its `AppBar`, scrollable
/// card list, and floating action button.
///
/// Split out of [`MaterialDemoRoot`] so the navigator shell above can seed it
/// as a route once, in `create_state`, rather than reconstructing it (and
/// losing its persistent state) on every `Navigator` rebuild.
#[derive(Clone, StatefulView)]
struct MaterialDemoHome {
    items: Rc<RefCell<Vec<String>>>,
    selected: Rc<RefCell<Option<String>>>,
    navigator: NavigatorHandle,
    /// Incremented once per [`MaterialDemoHomeState::create_state`] call —
    /// see the field doc on [`MaterialDemoRoot::home_create_count`], which
    /// owns the `Rc` this clones.
    create_count: Rc<Cell<u32>>,
}

/// Persistent state for [`MaterialDemoHome`].
///
/// Captures a `RebuildHandle` in `init_state` (ADR-0018) so a tap/press
/// callback — which runs outside `build`/layout/paint — can schedule the
/// next frame's rebuild without touching the tree itself.
struct MaterialDemoHomeState {
    items: Rc<RefCell<Vec<String>>>,
    selected: Rc<RefCell<Option<String>>>,
    navigator: NavigatorHandle,
    /// The list's live scroll position, fed straight into `ListView`
    /// (`ListView::position`) — see `vertical_slice_demo::DemoHomeState`'s
    /// matching field doc for the content-dimension feedback loop this
    /// enables.
    scroll_controller: ScrollController,
    /// `None` only before `init_state` has run; every `build` call happens
    /// after it (`ViewState` lifecycle order), so it is always `Some` there.
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for MaterialDemoHome {
    type State = MaterialDemoHomeState;

    fn create_state(&self) -> Self::State {
        // The discriminator itself — see `MaterialDemoRoot::home_create_count`'s doc.
        self.create_count.set(self.create_count.get() + 1);

        MaterialDemoHomeState {
            items: Rc::clone(&self.items),
            selected: Rc::clone(&self.selected),
            navigator: self.navigator.clone(),
            scroll_controller: ScrollController::new(),
            rebuild: None,
        }
    }
}

impl ViewState<MaterialDemoHome> for MaterialDemoHomeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
        // Lifecycle-only acquisition (ADR-0021, port-check trigger #22) — see
        // `vertical_slice_demo::DemoHomeState::init_state`'s matching comment
        // for why this is safe here and what it wires up.
        if let Some(handle) = ctx.post_frame_handle() {
            self.scroll_controller.position().set_flush_handle(handle);
        }
    }

    fn build(&self, _view: &MaterialDemoHome, ctx: &dyn BuildContext) -> impl IntoView {
        // A plain ambient lookup (`ctx.get`, no dependency registered) —
        // unlike `rebuild_handle`/`post_frame_handle`, safe to call from
        // `build` itself; see `ScaffoldMessengerScope::maybe_of`'s own doc.
        let messenger = ScaffoldMessengerScope::of(ctx);
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: init_state runs before build (ViewState lifecycle order)");

        let selected_text = match self.selected.borrow().as_ref() {
            Some(label) => format!("Selected: {label}"),
            None => "Selected: none".to_string(),
        };

        let items_snapshot = self.items.borrow().clone();
        let cards = items_snapshot
            .into_iter()
            .map(|label| {
                let selected_for_tap = Rc::clone(&self.selected);
                let rebuild_for_tap = rebuild.clone();
                let label_for_tap = label.clone();
                Card::new(
                    InkWell::new(Padding::new(EdgeInsets::all(px(12.0))).child(Text::new(label)))
                        .on_tap(move || {
                            selected_for_tap.borrow_mut().replace(label_for_tap.clone());
                            rebuild_for_tap.schedule();
                        }),
                )
                .boxed()
            })
            .collect::<Vec<_>>();

        // Drag-to-scroll: a plain `GestureDetector` feeding
        // `scroll_controller` directly — see the module doc's "body" bullet
        // for why this doesn't ride `Scrollable`.
        let scroll_controller_for_drag = self.scroll_controller.clone();
        let list = GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pan_update(move |details: DragUpdateDetails| {
                let proposed = scroll_controller_for_drag.pixels() - details.delta.dy.get();
                scroll_controller_for_drag.jump_to(proposed);
            })
            .child(ListView::new(ITEM_EXTENT, cards).position(self.scroll_controller.position()));

        let navigator_for_action = self.navigator.clone();
        let app_bar = AppBar::new()
            .title(Text::new(APP_TITLE))
            // The home route is the navigator's root — it never wants an
            // implied back button. Also sidesteps `AppBar`'s own documented
            // "second named divergence" (`app_bar.rs`'s module docs):
            // `NavigatorHandle::can_pop` is navigator-global, so once the
            // settings route is pushed, an `AppBar` left at its
            // `automatically_imply_leading` default would resolve the SAME
            // "can pop" answer as the settings route's own app bar and
            // synthesize a second, redundant `BackButton` here too.
            .automatically_imply_leading(false)
            .actions(vec![
                IconButton::new(Icon::new(settings_icon_data()))
                    .on_pressed(move || {
                        navigator_for_action.push(settings_route());
                    })
                    .boxed(),
            ]);

        let navigator_for_fab = self.navigator.clone();
        let items_for_fab = Rc::clone(&self.items);
        let rebuild_for_fab = rebuild;
        let messenger_for_fab = messenger;
        let fab = FloatingActionButton::new(
            Some(move || {
                open_add_item_dialog(
                    &navigator_for_fab,
                    Rc::clone(&items_for_fab),
                    rebuild_for_fab.clone(),
                    messenger_for_fab.clone(),
                );
            }),
            Text::new(FAB_LABEL),
        );

        Scaffold::new()
            .app_bar(app_bar)
            .floating_action_button(fab)
            .body(Column::new(column![
                Padding::new(EdgeInsets::all(px(8.0))).child(Text::new(selected_text)),
                Expanded::new(list),
            ]))
    }
}

/// Opens the "Add item" dialog: `Cancel` pops with no change, `Add` appends
/// a fresh item to `items`, schedules the home route's rebuild, pops, and
/// shows [`SNACK_BAR_ADDED_MESSAGE`] via `messenger` — the scope-mount
/// pattern's payoff: one call against the ambient handle, no `Scaffold`
/// plumbing at this call site.
///
/// `show_dialog` pushes a `PopupRoute` (`opaque: false`) — the home route
/// stays mounted underneath, so `rebuild`'s `RebuildHandle` (captured from
/// the home route's own `init_state`, before this dialog ever opened) is
/// still valid to schedule once the dialog closes.
fn open_add_item_dialog(
    navigator: &NavigatorHandle,
    items: Rc<RefCell<Vec<String>>>,
    rebuild: RebuildHandle,
    messenger: ScaffoldMessengerHandle,
) {
    let navigator_for_builder = navigator.clone();
    show_dialog::<(), _, _>(navigator, move |_ctx| {
        let navigator_for_cancel = navigator_for_builder.clone();
        let navigator_for_add = navigator_for_builder.clone();
        let items_for_add = Rc::clone(&items);
        let rebuild_for_add = rebuild.clone();
        let messenger_for_add = messenger.clone();

        AlertDialog::new()
            .title(Text::new(ADD_DIALOG_TITLE))
            .content(Text::new(ADD_DIALOG_CONTENT))
            .actions(vec![
                TextButton::new(Text::new(CANCEL_LABEL))
                    .on_pressed(move || {
                        navigator_for_cancel.pop();
                    })
                    .boxed(),
                FilledButton::new(Text::new(ADD_LABEL))
                    .on_pressed(move || {
                        let next_index = items_for_add.borrow().len();
                        items_for_add
                            .borrow_mut()
                            .push(format!("{ITEM_LABEL_PREFIX}{next_index}"));
                        navigator_for_add.pop();
                        rebuild_for_add.schedule();
                        messenger_for_add
                            .show_snack_bar(SnackBar::new(Text::new(SNACK_BAR_ADDED_MESSAGE)));
                    })
                    .boxed(),
            ])
    });
}

/// The settings route: a second `Scaffold`/`AppBar` page, pushed by the home
/// route's app bar action. No explicit `leading` is set, so `AppBar`
/// synthesizes a `BackButton` — the navigator has two entries by the time
/// this route builds, so `NavigatorHandle::can_pop` is `true`.
fn settings_route() -> PageRoute<()> {
    PageRoute::new(|_ctx, _animation, _secondary| {
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new(SETTINGS_ROUTE_TITLE)))
            .body(Center::new().child(Text::new(SETTINGS_ROUTE_TEXT)))
            .into_view()
            .boxed()
    })
    .named("settings")
}

/// Build a fresh demo tree, ready to mount.
#[must_use]
pub fn demo_root() -> MaterialDemoRoot {
    MaterialDemoRoot::new()
}

/// Thin `StatelessView` entry point for [`flui_app::run_app`](https://docs.rs/flui-app),
/// which requires a stateless root — the demo's actual state lives one level
/// down, in [`MaterialDemoRoot`]. Wraps the tree in `MediaQuery(default) →
/// Theme(ThemeData::light())` once, at the very root, so every route pushed
/// onto the shared navigator (the settings route, the dialog) stays a
/// structural descendant of both: `Scaffold`/`AppBar` call `MediaQuery::of`
/// unconditionally (panicking with no ancestor — `flui_app::run_app` installs
/// no `MediaQuery` of its own, unlike a full `WidgetsApp`/`MaterialApp`), and
/// `Theme::of` does the same for `ThemeData`. The acceptance test mounts this
/// same wrapped tree (see `tests/material_demo.rs`), so both consumers see
/// the identical composition.
#[derive(Clone, StatelessView)]
pub struct MaterialDemoApp;

impl StatelessView for MaterialDemoApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        MediaQuery::new(
            MediaQueryData::default(),
            Theme::new(ThemeData::light(), demo_root()),
        )
    }
}
