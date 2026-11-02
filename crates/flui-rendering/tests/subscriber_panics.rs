//! A tracing subscriber is user code. One that panics as a layout root's span opens or closes
//! leaves the owner able to lay out again.

use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::constraints::BoxConstraints;

/// Panics when a span named `name` opens (`on_close = false`) or exits (`on_close = true`).
struct PanicsOnSpan {
    name: &'static str,
    on_exit: bool,
}

impl tracing::Subscriber for PanicsOnSpan {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        if attributes.metadata().name() == self.name {
            assert!(self.on_exit, "a subscriber that cannot open {}", self.name);
            return tracing::span::Id::from_u64(2);
        }
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, id: &tracing::span::Id) {
        assert!(
            !(self.on_exit && id.into_u64() == 2),
            "a subscriber that cannot close {}",
            self.name
        );
    }
}

/// Lays the tree out under `subscriber`, expecting its panic, then asks for a larger root and
/// lays out again without it: the second layout must run, so the root takes the new size.
///
/// Fails when the first panic leaves the layout phase set or drops the drained batch: the second
/// mark is then routed to a queue nobody drains, and the root keeps its old size.
fn layout_survives(subscriber: PanicsOnSpan) {
    let closes = subscriber.on_exit;
    let (mut owner, root) = {
        let mut owner = flui_rendering::pipeline::PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        );
        let root = owner.set_root_render_object(Box::new(RenderPadding::all(1.0)));
        owner
            .insert_child_render_object(root, Box::new(RenderColoredBox::red(10.0, 10.0)))
            .expect("the child attaches");
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 100.0))));
        // One whole frame first, so the tree is laid out and painted and owes nothing; the
        // constraints change then dirties layout alone.
        let (mut owner, shown) = owner.run_frame();
        shown.expect("the first frame runs");
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(150.0, 150.0))));
        (owner.into_layout(), root)
    };
    // A second dispatcher keeps the callsites' interest from being cached against another
    // thread's empty default.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let unwound = tracing::subscriber::with_default(subscriber, || {
        tracing::callsite::rebuild_interest_cache();
        catch_unwind(AssertUnwindSafe(|| owner.run_layout()))
    });
    assert!(unwound.is_err(), "the subscriber's panic continues");
    assert!(!owner.debug_doing_layout(), "the layout phase was left");
    assert!(
        owner
            .nodes_needing_layout()
            .iter()
            .any(|node| node.id == root),
        "the root was dropped from the dirty queue"
    );
    if closes {
        // The root was laid out before the span closed, so its changed geometry still owes a
        // paint that the retained (now clean) layout entry would not bring back.
        assert!(
            !owner.nodes_needing_paint().is_empty(),
            "the laid-out root owes a paint"
        );
    }
    tracing::callsite::rebuild_interest_cache();

    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    owner.run_layout().expect("the owner lays out again");
    assert_eq!(
        owner.box_size(root),
        Some(Size::new(200.0, 200.0)),
        "the second layout ran"
    );
}

#[test]
fn a_subscriber_that_panics_as_a_layout_root_opens_leaves_the_owner_usable() {
    layout_survives(PanicsOnSpan {
        name: "layout_root",
        on_exit: false,
    });
}

#[test]
fn a_subscriber_that_panics_as_a_layout_root_closes_leaves_the_owner_usable() {
    layout_survives(PanicsOnSpan {
        name: "layout_root",
        on_exit: true,
    });
}
