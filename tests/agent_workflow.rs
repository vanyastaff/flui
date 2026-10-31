//! Reproducible "agent workflow" acceptance test: an agent can discover the
//! public API, create a UI, inspect structure/semantics, drive an
//! interaction, and assert the result — all through the facade's
//! *documented* interfaces, with no widget-internal shortcut and no
//! fabricated result.
//!
//! Everything below is reachable from `use flui::…` alone (the `flui` crate
//! with its `testing` feature on): no direct dependency on `flui-testing`,
//! `flui-widgets`, `flui-rendering`, or any other implementation crate. That
//! is the point of the test — it is written the way an outside consumer of
//! the published `flui` crate would have to write it, not the way this
//! workspace's own internal tests do (compare
//! `packages/flui-material/tests/elevated_button.rs`, which reaches past
//! the facade into `flui_rendering`/`flui_interaction` directly, and
//! drives taps through a widget-testing convenience
//! (`flui_testing::widgets::lay_out`'s `dispatch_pointer_down`) rather than
//! the general pointer-replay path this test uses).
//!
//! ## The five steps, and which public API answers each one
//!
//! 1. **Mount** — [`flui::testing::HeadlessBinding::mount_root`], the same
//!    canonical bootstrap every headless harness in this workspace shares.
//! 2. **Inspect structure** — [`flui::testing::rendering::render_snapshot`]
//!    over the bound [`flui::testing::HeadlessBinding::pipeline_owner`],
//!    read by type through [`flui::foundation::inspect::SnapshotNode`].
//!    (The facade re-exports `render_snapshot` for exactly this.)
//! 3. **Inspect semantics** — [`flui::testing::HeadlessBinding::enable_semantics`]
//!    and [`flui::testing::HeadlessBinding::a11y_tree`], queried with the
//!    tree's [`find_by_label`](flui::testing::a11y::A11yTree::find_by_label).
//! 4. **Drive** — [`flui::testing::HeadlessBinding::replay`] with a scripted
//!    [`flui::testing::replay::PointerScript::tap`] at the semantics node's
//!    own bounds — the same `GestureBinding` input pipeline production code
//!    routes through, not a widget-tree shortcut.
//! 5. **Assert** — the render snapshot again, checked for the
//!    `RenderParagraph` `text` field advancing from `0` to `1`: the
//!    rendered output, not an internal counter field.
//!
//! ## What step 3 relies on
//!
//! The CLI `counter` template's tree (`Center` → `Column` →
//! `Text`/`Text`/`ElevatedButton`, mirrored below) carries no explicit
//! `Semantics` node. The label step 3 finds comes from the framework itself:
//! `flui-material`'s `ButtonStyleButtonCore` publishes a button semantics
//! node, and `RenderParagraph` publishes its text as that node's label (see
//! `crates/flui-objects/ARCHITECTURE.md`, "Semantics mapping"). Writing the
//! first version of this test is what found that neither did — the counter
//! app was not screen-reader accessible out of the box, and this test had
//! to wrap the button in an explicit `Semantics` to have anything to query.
//! It no longer does; the tree below IS the template's.

use std::time::Duration;

use flui::foundation::inspect::SnapshotNode;
use flui::geometry::Offset;
use flui::prelude::*;
use flui::testing::rendering::render_snapshot;
use flui::testing::replay::PointerScript;
use flui::testing::{HeadlessBinding, MountOptions, MountOwners};
use flui::widgets::column;

/// The tree `flui create`'s counter template builds: `Center` → `Column` →
/// prompt `Text` / count `Text` / `ElevatedButton`, driven by a `StateCell`
/// bound in `init_state` — see `crates/flui-view/src/state_cell.rs`. No
/// additions: the semantics step 3 queries are the framework's own.
#[derive(Clone, StatefulView)]
struct AgentCounter;

struct AgentCounterState {
    count: StateCell<usize>,
}

impl StatefulView for AgentCounter {
    type State = AgentCounterState;

    fn create_state(&self) -> Self::State {
        AgentCounterState {
            count: StateCell::new(0),
        }
    }
}

impl ViewState<AgentCounter> for AgentCounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count.bind(ctx);
    }

    fn build(&self, _view: &AgentCounter, _ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.clone();

        Center::new().child(
            Column::new(column![
                Text::new("You have pushed the button this many times:"),
                SizedBox::height(16.0),
                Text::new(self.count.get().to_string()),
                SizedBox::height(16.0),
                ElevatedButton::new(Text::new("Increment"))
                    .on_pressed(move |_cx| count.update(|n| n + 1)),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}

/// Mounts [`AgentCounter`] through the canonical headless bootstrap, wrapped
/// exactly as `flui_testing::widgets::lay_out` wraps every widget-tier test
/// (`GestureArenaScope` for tap routing, `FocusRoot` because a mounted tree
/// always needs a focus scope, `Theme` because `ElevatedButton` panics
/// without one — see `flui_material::Theme::of`'s doc comment).
///
/// Returns the bound `binding`; the caller drives frames and queries it from
/// there. This is the facade-only equivalent of what `mount_root`'s own doc
/// example shows, applied to a tree that actually needs presentation scopes.
fn mount_agent_counter() -> HeadlessBinding {
    let mut binding = HeadlessBinding::new();
    let root = GestureArenaScope::new(
        binding.arena().clone(),
        FocusRoot::new(Theme::new(ThemeData::light(), AgentCounter)),
    );
    let mounted = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(480.0, 320.0),
    );
    assert!(
        mounted.painted,
        "the bootstrap frame must commit a paint before anything below can be inspected"
    );
    binding
}

/// Takes the typed snapshot of the current render tree — step 2's tool, reused for step 5's
/// assertion too, since both are "what does the render tree actually say".
fn render_dump(binding: &HeadlessBinding) -> SnapshotNode {
    let pipeline = binding
        .pipeline_owner()
        .expect("mount_agent_counter always returns a tree-bound binding");
    pipeline.with(render_snapshot)
}

/// The plain text of every `RenderParagraph` in `tree`, in tree order, read as the typed
/// `text` field — an exact match per paragraph, so `"1"` cannot match a paragraph that
/// says `"10"`.
fn paragraph_texts(tree: &SnapshotNode) -> Vec<&str> {
    tree.find_by_kind("RenderParagraph")
        .as_slice()
        .iter()
        .filter_map(|paragraph| paragraph.get::<&str>("text").ok())
        .collect()
}

#[test]
fn agent_can_mount_inspect_drive_and_assert_the_counter() {
    // --- 1. Mount: the counter UI, through the documented bootstrap. -------
    let mut binding = mount_agent_counter();
    // --- 2. Inspect structure: snapshot the render tree and check a
    // --- 2. Inspect structure: dump the render-tree diagnostics and check a
    // known node is really there. `ElevatedButton` composes a Material
    // surface as a `RenderPhysicalShape` (see
    // `packages/flui-material/tests/elevated_button.rs`, which asserts the
    // same node by the same name) — a fact about the framework's own
    // composition, not something this test invents.
    let initial = render_dump(&binding);
    assert!(
        initial
            .iter()
            .any(|node| node.kind == "RenderPhysicalShape"),
        "expected a RenderPhysicalShape node (ElevatedButton's Material surface) \
         in the render tree, got paragraphs {:?}",
        paragraph_texts(&initial)
    );
    assert!(
        paragraph_texts(&initial).contains(&"0"),
        "the counter must render its initial value \"0\"; paragraphs were {:?}",
        paragraph_texts(&initial)
    );

    // --- 3. Inspect semantics: find the button by its label. ---------------
    // Semantics assembly is opt-in and lazy (see `enable_semantics`'s doc
    // comment) — enable it, then run one more frame so it actually gets
    // built before `a11y_tree` is read.
    binding
        .enable_semantics()
        .expect("mount_agent_counter returns a tree-bound binding");
    binding.pump_frame(Duration::from_millis(16));
    let tree = binding
        .a11y_tree()
        .expect("semantics was enabled and a frame ran, so a tree must exist");
    let button = tree
        .find_by_label("Increment")
        .expect("ElevatedButton publishes a button node labelled by its Text child");
    let bounds = button
        .bounds()
        .expect("a laid-out node always carries bounds");

    // --- 4. Drive: tap the center of those bounds through the public
    // pointer-replay path (`HeadlessBinding::replay`), not a widget-testing
    // shortcut like `dispatch_pointer_down`/`find_text`.
    let center = Offset::new(bounds.x0.midpoint(bounds.x1), bounds.y0.midpoint(bounds.y1));
    binding.replay(&PointerScript::tap(center));
    // The replay's own doc is explicit that the frame after the last
    // scripted event is the caller's to run: the tap's up-event schedules a
    // rebuild, which only a subsequent frame drains.
    binding.pump_frame(Duration::from_millis(16));

    // --- 5. Assert: the rendered text advanced 0 -> 1. ----------------------
    let last = render_dump(&binding);
    let texts = paragraph_texts(&last);
    assert!(
        texts.contains(&"1"),
        "a tap on the Increment button must advance the rendered count to \"1\"; \
         paragraphs were {texts:?}"
    );
    assert!(
        !texts.contains(&"0"),
        "the old rendered count must not remain after the tap; paragraphs were {texts:?}"
    );
}
