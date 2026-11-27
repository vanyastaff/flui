//! Reproducible "agent workflow" acceptance test: an agent can discover the
//! public API, create a UI, inspect its structure and semantics, drive an
//! interaction, and assert the result, all through the facade's documented
//! interfaces with no widget-internal shortcut and no fabricated result.
//!
//! Everything below is reachable from `use flui::…` alone (the `flui` crate
//! with its `testing` and `material` features on): it is written the way an
//! outside consumer of the published crate writes it.
//!
//! ## The five steps, and which public API answers each one
//!
//! 1. **Mount**: [`flui::testing::scenario::Tester::mount`], which mounts the
//!    tree as `run_app` does.
//! 2. **Inspect structure**: the old tester's `inspect_render`, the render tree
//!    as the agent's `inspect.render` reads it (the `render_tree` module,
//!    until the scenario tester reads it).
//! 3. **Inspect semantics**: [`flui::testing::scenario::find::button`], a
//!    `Find` over the semantics tree, read with
//!    [`flui::testing::scenario::Tester::read`].
//! 4. **Drive**: [`flui::testing::scenario::Tester::tap`], which checks the
//!    button is visible, enabled and not covered, then delivers a press and
//!    release through the window's own input path, as the driver does.
//! 5. **Assert**: [`flui::testing::scenario::Tester::expect`] on the shown
//!    count, and the render tree again, checked for the `RenderParagraph`
//!    whose `text` field advanced from `0` to `1`: the rendered output, not
//!    an internal counter field.
//!
//! The tree carries no explicit `Semantics` node: `ElevatedButton` publishes a
//! button node labelled by its `Text` (see `crates/flui-objects/ARCHITECTURE.md`,
//! "Semantics mapping").

use flui::prelude::*;
use flui::testing::Role;
use flui::testing::scenario::matcher::{enabled, exists, gone};
use flui::testing::scenario::{Tester, find};

#[derive(Clone, StatefulView)]
struct AgentCounter;

struct AgentCounterState {
    count: Signal<usize>,
}

impl StatefulView for AgentCounter {
    type State = AgentCounterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        AgentCounterState {
            count: ctx.signal(0),
        }
    }
}

impl ViewState<AgentCounter> for AgentCounterState {
    fn build(&self, _view: &AgentCounter, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;

        Center::new().child(
            Column::new((
                Text::new("You have pushed the button this many times:"),
                SizedBox::height(16.0),
                Text::new(count.get(ctx).to_string()),
                SizedBox::height(16.0),
                ElevatedButton::new(Text::new("Increment"))
                    .on_pressed(move |ctx| count.update(ctx, |n| *n += 1)),
            ))
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}

#[test]
fn agent_can_mount_read_drive_and_assert_the_counter() {
    // 1. Mount: `ElevatedButton` reads its colours from a `Theme`.
    let mut t = Tester::mount(Theme::new(ThemeData::light(), AgentCounter));
    t.expect(&find::text("0"), exists());

    // 3. Inspect semantics: the button, by its role and the label its text gives it.
    let button = find::button("Increment");
    assert_eq!(t.read(&button).role, Role::Button);
    t.expect(&button, enabled());

    // 4. Drive: a tap through the window's input path.
    t.tap(&button);

    // 5. Assert: the shown count advanced and the old one is gone.
    t.expect(&find::text("1"), exists());
    t.expect(&find::text("0"), gone());
    t.finish();
}

/// The render tree has no counterpart in the scenario tester's semantics reads, so the
/// structure and the painted text are read through the old tester's `inspect_render`.
#[expect(
    deprecated,
    reason = "ADR-0400: `scenario::Tester` has no render-tree read yet; this module moves with it"
)]
mod render_tree {
    use flui::prelude::*;
    use flui::testing::{InspectNode, InspectValue, Tester, find};

    use super::AgentCounter;

    /// Every node of `roots`, depth first.
    fn nodes(roots: &[InspectNode]) -> Vec<&InspectNode> {
        let mut out = Vec::new();
        let mut stack: Vec<&InspectNode> = roots.iter().rev().collect();
        while let Some(node) = stack.pop() {
            out.push(node);
            stack.extend(node.children.iter().rev());
        }
        out
    }

    /// The `text` field of every `RenderParagraph` in the render tree, in tree order.
    fn paragraph_texts(t: &mut Tester) -> Vec<String> {
        let reading = t.inspect_render();
        nodes(&reading.roots)
            .into_iter()
            .filter(|node| node.kind == "RenderParagraph")
            .filter_map(|node| {
                node.fields.iter().find_map(|field| match &field.value {
                    InspectValue::Str(text) if field.name == "text" => Some(text.clone()),
                    _ => None,
                })
            })
            .collect()
    }

    #[test]
    fn agent_can_inspect_the_render_tree_before_and_after_a_tap() {
        let mut t = Tester::mount(Theme::new(ThemeData::light(), AgentCounter));

        // `ElevatedButton` composes a Material surface as a `RenderPhysicalShape`.
        let reading = t.inspect_render();
        assert!(
            nodes(&reading.roots)
                .iter()
                .any(|node| node.kind == "RenderPhysicalShape"),
            "expected ElevatedButton's RenderPhysicalShape in the render tree"
        );
        let texts = paragraph_texts(&mut t);
        assert!(
            texts.iter().any(|text| text == "0"),
            "paragraphs were {texts:?}"
        );

        t.tap(&find::button("Increment"));

        let texts = paragraph_texts(&mut t);
        assert!(
            texts.iter().any(|text| text == "1"),
            "paragraphs were {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "0"),
            "the old count must not remain; paragraphs were {texts:?}"
        );
    }
}
