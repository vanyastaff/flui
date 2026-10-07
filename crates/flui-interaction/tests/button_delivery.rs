//! Button edges preserve the chronological contact route around queued movement.

use std::{cell::RefCell, rc::Rc};

use flui_interaction::{GestureBinding, HitTestEntry, HitTestResult, InteractionLane, RenderId};
use flui_platform_api::{
    EventTime,
    pointer::{
        ButtonChange, DeviceId, PointerButton, PointerButtons, PointerEvent, PointerId,
        PointerInfo, PointerKind, PointerMove, PointerPosition, PointerPress, PointerRelease,
        PointerRole, PointerSample,
    },
};

#[test]
fn queued_move_precedes_button_edges_without_restarting_the_contact() {
    let lane = InteractionLane::try_new().expect("owner lane");
    let binding = GestureBinding::new();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&observed);
    lane.enter(|| {
        let target = lane
            .dispatch_handle()
            .register_pointer(move |dispatch| {
                let record = match dispatch.local {
                    PointerEvent::Down(event) => (
                        "down",
                        event.sample.time.as_nanos(),
                        event.buttons(),
                        Vec::new(),
                    ),
                    PointerEvent::Move(event) => (
                        "move",
                        event.current().time.as_nanos(),
                        event.buttons,
                        event
                            .coalesced()
                            .iter()
                            .map(|sample| sample.time.as_nanos())
                            .collect(),
                    ),
                    PointerEvent::ButtonChange(ButtonChange::Pressed(event)) => (
                        "press",
                        event.sample.time.as_nanos(),
                        event.buttons(),
                        Vec::new(),
                    ),
                    PointerEvent::ButtonChange(ButtonChange::Released(event)) => (
                        "release",
                        event.sample.time.as_nanos(),
                        event.buttons(),
                        Vec::new(),
                    ),
                    PointerEvent::Up(event) => (
                        "up",
                        event.sample.time.as_nanos(),
                        event.buttons(),
                        Vec::new(),
                    ),
                    _ => return,
                };
                log.borrow_mut().push(record);
            })
            .expect("register listener route");
        let route = |_| {
            let mut path = HitTestResult::new();
            path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
            path
        };
        let primary = PointerButtons::only(PointerButton::PRIMARY);
        let both = primary.with(PointerButton::SECONDARY);
        for (identity, base) in [(1, 0), (2, 40)] {
            observed.borrow_mut().clear();
            let pointer = PointerInfo::new(
                PointerId::try_from(identity).expect("nonzero contact"),
                PointerKind::Mouse,
            )
            .with_device(DeviceId::try_from(1).expect("nonzero hardware"))
            .with_role(PointerRole::Primary);
            let sample = |millis| {
                PointerSample::new(
                    EventTime::from_nanos((base + millis) * 1_000_000),
                    PointerPosition::try_new(flui_foundation::geometry::Point::new(
                        millis as f64,
                        0.0,
                    ))
                    .expect("finite coordinate"),
                )
            };
            binding.handle_pointer_event(
                &PointerEvent::Down(PointerPress::new(
                    pointer,
                    PointerButton::PRIMARY,
                    primary,
                    sample(0),
                )),
                route,
            );
            binding.handle_pointer_event(
                &PointerEvent::Move(
                    PointerMove::new(pointer, primary, sample(10)).with_coalesced(vec![sample(5)]),
                ),
                route,
            );
            assert_eq!(
                observed.borrow().len(),
                1,
                "the movement awaits frame delivery"
            );
            binding.handle_pointer_event(
                &PointerEvent::ButtonChange(ButtonChange::Pressed(PointerPress::new(
                    pointer,
                    PointerButton::SECONDARY,
                    both,
                    sample(20),
                ))),
                route,
            );
            assert_eq!(
                binding.active_pointer_count(),
                1,
                "another button does not create or finish a contact"
            );
            assert_eq!(
                &*observed.borrow(),
                &[
                    ("down", base * 1_000_000, primary, vec![]),
                    (
                        "move",
                        (base + 10) * 1_000_000,
                        primary,
                        vec![(base + 5) * 1_000_000]
                    ),
                    ("press", (base + 20) * 1_000_000, both, vec![]),
                ],
                "an edge cannot overtake the earlier movement or rewrite its samples/buttons"
            );
            binding.handle_pointer_event(
                &PointerEvent::ButtonChange(ButtonChange::Released(PointerRelease::new(
                    pointer,
                    PointerButton::SECONDARY,
                    primary,
                    sample(25),
                ))),
                route,
            );
            assert_eq!(binding.active_pointer_count(), 1);
            binding.handle_pointer_event(
                &PointerEvent::Up(PointerRelease::new(
                    pointer,
                    PointerButton::PRIMARY,
                    PointerButtons::NONE,
                    sample(30),
                )),
                route,
            );
            binding.flush_pending_moves();
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(
                &observed.borrow()[3..],
                &[
                    ("release", (base + 25) * 1_000_000, primary, vec![]),
                    ("up", (base + 30) * 1_000_000, PointerButtons::NONE, vec![]),
                ],
                "the terminal event leaves no queued movement debt"
            );
        }
    });
}
