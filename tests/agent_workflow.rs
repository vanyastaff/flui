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
//! workspace's own internal tests do. In particular, interaction goes through
//! the general pointer-replay path rather than a widget-tree shortcut.
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
//! The included CLI `counter` template's tree (`SafeArea` → `Center` →
//! `Column` → `Text`/`Text`/`RawButton`) adds no test-only `Semantics` node.
//! `RawButton` itself publishes a button semantics container, and the child
//! `RenderParagraph` publishes its text as that node's label (see
//! `crates/flui-objects/ARCHITECTURE.md`, "Semantics mapping").

use std::time::Duration;

use flui::foundation::inspect::SnapshotNode;
use flui::geometry::{Offset, Size};
use flui::prelude::*;
use flui::testing::a11y::Role;
use flui::testing::rendering::render_snapshot;
use flui::testing::replay::PointerScript;
use flui::testing::{HeadlessBinding, MountOptions, MountOwners};
use flui::view::environment::{FocusTreeRoot, GestureArenaScope};

mod counter_app {
    include!("../crates/flui-cli/src/templates/counter_app.rs");
}

use counter_app::CounterApp;

/// Mounts the real template [`CounterApp`] through the canonical headless
/// bootstrap. `MediaQuery` supplies the production surface data `SafeArea`
/// reads, `GestureArenaScope` routes taps, and `FocusTreeRoot` supplies the
/// focus scope `RawButton` needs.
///
/// Returns the bound `binding`; the caller drives frames and queries it from
/// there. This is the facade-only equivalent of what `mount_root`'s own doc
/// example shows, applied to a tree that actually needs presentation scopes.
fn mount_agent_counter() -> HeadlessBinding {
    const WIDTH: f64 = 480.0;
    const HEIGHT: f64 = 320.0;

    let mut binding = HeadlessBinding::new();
    let app = MediaQuery::new(
        MediaQueryData {
            size: Size::new(WIDTH, HEIGHT),
            ..MediaQueryData::default()
        },
        CounterApp,
    );
    let root = GestureArenaScope::new(binding.arena().clone(), FocusTreeRoot::new(app));
    let mounted = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(WIDTH, HEIGHT),
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
    // --- 1. Mount: the real template counter, through the documented bootstrap.
    let mut binding = mount_agent_counter();

    // --- 2. Inspect structure: the source's three paragraphs are present, and
    // RawButton has an annotated semantics container in the actual render tree.
    let initial = render_dump(&binding);
    assert_eq!(
        paragraph_texts(&initial),
        vec![
            "You have pushed the button this many times:",
            "0",
            "Increment",
        ],
        "the initial render tree must be the real counter template"
    );
    assert!(
        initial.iter().any(|node| {
            node.kind == "RenderSemanticsAnnotations"
                && node.get::<bool>("container") == Ok(true)
                && node.get::<bool>("has_semantics") == Ok(true)
        }),
        "expected RawButton's annotated semantics container in the render tree"
    );

    // --- 3. Inspect semantics: find the enabled button by its label. --------
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
        .expect("RawButton publishes a button node labelled by its Text child");
    assert_eq!(
        button.role(),
        Role::Button,
        "RawButton must expose its button role. Tree was:\n{}",
        tree.describe()
    );
    assert!(
        !button.is_disabled(),
        "the counter's RawButton has an on_press callback and must be enabled. Tree was:\n{}",
        tree.describe()
    );
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
