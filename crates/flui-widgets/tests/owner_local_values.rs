//! Owner-local values fit the render-layer positions a widget author fills:
//! hit-test metadata and semantics action handlers. Each value captures an
//! `Rc<Cell<_>>`, so a `Send` bound returning to either position stops this
//! module from compiling. The facade consumer
//! (`owner_local_values_fit_every_flip_position`) covers the widget-layer
//! positions.

use std::cell::Cell;
use std::rc::Rc;

use flui_rendering::hit_testing::HitMetadata;
use flui_rendering::semantics::{ActionCall, SemanticsAction, SemanticsConfiguration};
use flui_widgets::{WidgetStateProperty, WidgetStatesController};

static_assertions::assert_not_impl_any!(WidgetStateProperty<u32>: Send, Sync);
static_assertions::assert_not_impl_any!(WidgetStatesController: Send, Sync);
static_assertions::assert_not_impl_any!(SemanticsConfiguration: Send, Sync);

#[test]
fn owner_local_values_fit_hit_metadata_and_semantics_actions() {
    let shared = Rc::new(Cell::new(0_u32));

    let metadata = HitMetadata::new(shared.clone());
    let recovered = metadata
        .downcast_ref::<Rc<Cell<u32>>>()
        .expect("the value it was made with");
    assert!(Rc::ptr_eq(recovered, &shared));
    assert!(metadata.ptr_eq(&metadata.clone()));
    assert!(metadata.downcast_ref::<u32>().is_none());

    let mut configuration = SemanticsConfiguration::new();
    let clicks = shared.clone();
    configuration.add_action(
        SemanticsAction::Click,
        Rc::new(move |_call: ActionCall| clicks.set(clicks.get() + 1)),
    );
    let handler = configuration
        .action_handler(SemanticsAction::Click)
        .expect("the handler just added");
    handler(ActionCall::Click);
    assert_eq!(shared.get(), 1);
}
