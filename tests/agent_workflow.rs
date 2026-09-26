//! Reproducible "agent workflow" acceptance test (`docs/BETA.md`'s "Agent
//! workflow" row): an agent can discover the public API, create a UI, inspect
//! structure/semantics, drive an interaction, and assert the result — all
//! through the facade's *documented* interfaces, with no widget-internal
//! shortcut and no fabricated result.
//!
//! Everything below is reachable from `use flui::…` alone (the `flui` crate
//! with its `testing` feature on): no direct dependency on `flui-testing`,
//! `flui-widgets`, `flui-rendering`, or any other implementation crate. That
//! is the point of the test — it is written the way an outside consumer of
//! the published `flui` crate would have to write it, not the way this
//! workspace's own internal tests do (compare `tests/material_demo.rs` or
//! `packages/flui-material/tests/elevated_button.rs`, both of which reach past
//! the facade into `flui_rendering`/`flui_interaction` directly, and both of
//! which drive taps through a widget-testing convenience
//! (`flui_widgets::testing::lay_out`'s `dispatch_pointer_down`) rather than
//! the general pointer-replay path this test uses).
//!
//! ## The five steps, and which public API answers each one
//!
//! 1. **Mount** — [`flui::testing::HeadlessBinding::mount_root`], the same
//!    canonical bootstrap every headless harness in this workspace shares.
//! 2. **Inspect structure** — [`flui::testing::rendering::render_diagnostics`]
//!    over the bound [`flui::testing::HeadlessBinding::pipeline_owner`],
//!    rendered with [`flui::foundation::DiagnosticsNode::to_string_deep`].
//!    (Writing this test found that the facade did not re-export
//!    `render_diagnostics`; it does now.)
//! 3. **Inspect semantics** — [`flui::testing::HeadlessBinding::enable_semantics`]
//!    and [`flui::testing::HeadlessBinding::a11y_tree`], queried with the
//!    tree's [`find_by_label`](flui::testing::a11y::A11yTree::find_by_label).
//! 4. **Drive** — [`flui::testing::HeadlessBinding::replay`] with a scripted
//!    [`flui::testing::replay::PointerScript::tap`] at the semantics node's
//!    own bounds — the same `GestureBinding` input pipeline production code
//!    routes through, not a widget-tree shortcut.
//! 5. **Assert** — the render diagnostics dump again, checked for the
//!    `RenderParagraph` "text" property advancing from `0` to `1`: the
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

use flui::prelude::*;
use flui::testing::a11y::A11yQueryError;
use flui::testing::rendering::render_diagnostics;
use flui::testing::replay::PointerScript;
use flui::testing::{HeadlessBinding, MountOptions, MountOwners};
use flui::types::geometry::Offset;
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
                    .on_pressed(move || count.update(|n| n + 1)),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}

/// Mounts [`AgentCounter`] through the canonical headless bootstrap, wrapped
/// exactly as `flui_widgets::testing::lay_out` wraps every widget-tier test
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

/// Renders the current render-tree diagnostics as the deep-dump string an
/// agent (or a human) would read — step 2's tool, reused for step 5's
/// assertion too, since both are "what does the render tree actually say".
///
fn render_dump(binding: &HeadlessBinding) -> String {
    let pipeline = binding
        .pipeline_owner()
        .expect("mount_agent_counter always returns a tree-bound binding");
    pipeline.with(render_diagnostics).to_string_deep()
}

/// Whether `dump` contains a diagnostics property line matching exactly
/// `key: value` — a whole-line match rather than a substring one, so
/// `"text: 1"` cannot accidentally match a dump that actually says
/// `"text: 10"` or `"max_lines: 1"`.
fn has_property_line(dump: &str, key: &str, value: &str) -> bool {
    dump.lines()
        .any(|line| line.trim() == format!("{key}: {value}"))
}

#[test]
fn agent_can_mount_inspect_drive_and_assert_the_counter() {
    // --- 1. Mount: the counter UI, through the documented bootstrap. -------
    let mut binding = mount_agent_counter();

    // --- 2. Inspect structure: dump the render-tree diagnostics and check a
    // known node is really there. `ElevatedButton` composes a Material
    // surface as a `RenderPhysicalShape` (see
    // `packages/flui-material/tests/elevated_button.rs`, which asserts the
    // same node by the same name) — a fact about the framework's own
    // composition, not something this test invents.
    let initial_dump = render_dump(&binding);
    assert!(
        initial_dump
            .lines()
            .any(|line| line.trim() == "RenderPhysicalShape"),
        "expected a RenderPhysicalShape node (ElevatedButton's Material surface) \
         in the diagnostics dump, got:\n{initial_dump}"
    );
    assert!(
        has_property_line(&initial_dump, "text", "0"),
        "the counter must render its initial value \"0\"; dump was:\n{initial_dump}"
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
    let center = Offset::new(
        flui::types::geometry::px(bounds.x0.midpoint(bounds.x1) as f32),
        flui::types::geometry::px(bounds.y0.midpoint(bounds.y1) as f32),
    );
    binding.replay(&PointerScript::tap(center));
    // The replay's own doc is explicit that the frame after the last
    // scripted event is the caller's to run: the tap's up-event schedules a
    // rebuild, which only a subsequent frame drains.
    binding.pump_frame(Duration::from_millis(16));

    // --- 5. Assert: the rendered text advanced 0 -> 1. ----------------------
    let final_dump = render_dump(&binding);
    assert!(
        has_property_line(&final_dump, "text", "1"),
        "a tap on the Increment button must advance the rendered count to \"1\"; \
         dump was:\n{final_dump}"
    );
    assert!(
        !has_property_line(&final_dump, "text", "0"),
        "the old rendered count must not remain after the tap; dump was:\n{final_dump}"
    );
}

/// The failure-mode-is-actionable half of the acceptance criterion: an agent
/// that queries a label which is not in the tree must be told what it
/// searched for and what *was* available, not just "not found".
///
/// `A11yTree::find_by_label` already reports exactly that (see
/// `crates/flui-testing/src/a11y.rs`'s `A11yQueryError::NotFound`), so no new
/// helper was needed in `flui-testing` — this test is evidence that the
/// existing error type satisfies the acceptance criterion, not a helper it
/// had to add. If it turns out one *had* to be added, it would live at
/// `crates/flui-testing/src/a11y.rs` next to `A11yQueryError`.
#[test]
fn missing_label_query_reports_the_search_and_the_available_labels() {
    let mut binding = mount_agent_counter();
    binding
        .enable_semantics()
        .expect("mount_agent_counter returns a tree-bound binding");
    binding.pump_frame(Duration::from_millis(16));
    let tree = binding
        .a11y_tree()
        .expect("semantics was enabled and a frame ran, so a tree must exist");

    let error = tree
        .find_by_label("Decrement")
        .expect_err("there is no \"Decrement\" control in this tree");

    assert!(
        matches!(error, A11yQueryError::NotFound { .. }),
        "a query for a label that is not in the tree must fail with NotFound, got: {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains("Decrement"),
        "the error must name what was searched for (\"Decrement\"); message was:\n{message}"
    );
    assert!(
        message.contains("Increment"),
        "the error must list what labels ARE reachable (\"Increment\"), so the next \
         query an agent writes can be right instead of another guess; message was:\n{message}"
    );
}
