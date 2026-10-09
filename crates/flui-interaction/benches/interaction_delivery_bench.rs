//! Registered pointer, cursor and focus callback delivery on healthy owner-local paths.
//! Reusable owners, registrations, geometry and events are created outside timing.
//! Cursor/focus rows each time two real transitions, never same-state dedup.
//! No subscriber is installed: diagnostic containment is priced with tracing disabled.

use std::{cell::Cell, hint::black_box, rc::Rc};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::geometry::Offset;
use flui_interaction::events::{PointerButtons, PointerEvent, PointerKind, make_move_event_for_id};
use flui_interaction::routing::{
    DeviceId, FocusManager, FocusNode, FocusRequestOutcome, MouseTracker, PointerMotionKind,
    PointerRouteHandler, PointerRouter,
};
use flui_interaction::{CursorIcon, HitTestEntry, HitTestResult, PointerId, RenderId};

fn pointer() -> PointerId {
    PointerId::try_from(41_u64).expect("nonzero fixture pointer")
}

fn hover(position: Offset) -> PointerEvent {
    let mut event = make_move_event_for_id(pointer(), position, PointerKind::Mouse)
        .expect("finite fixture position");
    if let PointerEvent::Move(update) = &mut event {
        update.buttons = PointerButtons::NONE;
        update.pointer = update
            .pointer
            .with_device(DeviceId::try_from(41_u64).expect("nonzero fixture device"));
    }
    event
}

fn cursor_path(cursor: CursorIcon) -> HitTestResult {
    let mut path = HitTestResult::new();
    path.add(HitTestEntry::new(RenderId::new(1)).cursor(cursor));
    path
}

fn bench_registered_pointer_delivery(c: &mut Criterion) {
    let mut group = c.benchmark_group("delivery/PointerRouter/registered_move");
    let event = hover(Offset::new(10.0, 20.0));
    for count in [1_usize, 4] {
        let router = PointerRouter::new();
        let delivered = Rc::new(Cell::new(0_usize));
        let mut owners: Vec<PointerRouteHandler> = Vec::new();
        for _ in 0..count {
            let seen = delivered.clone();
            let callback: PointerRouteHandler = Rc::new(move |event| {
                black_box(event);
                seen.set(seen.get() + 1);
            });
            router.add_route(pointer(), callback.clone());
            owners.push(callback);
        }
        router.route(&event);
        assert_eq!(
            delivered.get(),
            count,
            "every registered owner receives Move"
        );
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                router.route(black_box(&event));
                black_box(delivered.get());
            });
        });
        black_box(&owners);
    }
    group.finish();
}

fn bench_cursor_changes(c: &mut Criterion) {
    let text = cursor_path(CursorIcon::Text);
    let arrow = cursor_path(CursorIcon::Default);
    let events = [
        hover(Offset::new(10.0, 20.0)),
        hover(Offset::new(20.0, 20.0)),
    ];
    for ambient in [false, true] {
        let tracker = MouseTracker::new();
        let changes = Rc::new(Cell::new(0_usize));
        let text_changes = Rc::new(Cell::new(0_usize));
        let observed = Rc::new(Cell::new(CursorIcon::Default));
        let (calls, last) = (changes.clone(), observed.clone());
        let texts = text_changes.clone();
        tracker.set_cursor_change_callback(Rc::new(move |source, cursor| {
            black_box(source);
            calls.set(calls.get() + 1);
            if cursor == CursorIcon::Text {
                texts.set(texts.get() + 1);
            }
            last.set(cursor);
        }));
        // Admit the physical device once. Each measured pair returns to Arrow.
        tracker.update_with_motion(&events[0], PointerMotionKind::Hover, &arrow);
        changes.set(0);
        let pair = || {
            if ambient {
                // Returning owned hit geometry matches the public refresh contract;
                // its one-entry path clone is intentionally included in this row.
                tracker.update_all_devices(|_| text.clone());
                tracker.update_all_devices(|_| arrow.clone());
            } else {
                tracker.update_with_motion(&events[0], PointerMotionKind::Hover, &text);
                tracker.update_with_motion(&events[1], PointerMotionKind::Hover, &arrow);
            }
        };
        pair();
        assert_eq!(
            changes.get(),
            2,
            "Text then Arrow are actual callback deliveries"
        );
        assert_eq!(observed.get(), CursorIcon::Default);
        assert_eq!(text_changes.get(), 1, "the other callback delivered Text");
        let name = if ambient {
            "delivery/MouseTracker/ambient_two_cursor_changes_owned_hit_paths"
        } else {
            "delivery/MouseTracker/physical_two_cursor_changes"
        };
        c.bench_function(name, |b| {
            b.iter(|| {
                pair();
                black_box((changes.get(), text_changes.get(), observed.get()));
            });
        });
    }
}

fn bench_focus_delivery(c: &mut Criterion) {
    let manager = FocusManager::new();
    let nodes = [FocusNode::new(), FocusNode::new()];
    let _attachments = [
        manager
            .root_scope()
            .attach_node(&nodes[0])
            .expect("first focus owner"),
        manager
            .root_scope()
            .attach_node(&nodes[1])
            .expect("second focus owner"),
    ];
    let deliveries = Rc::new(Cell::new(0_usize));
    let calls = deliveries.clone();
    let _listener = manager.add_listener(Rc::new(move |previous, current| {
        black_box((previous, current));
        calls.set(calls.get() + 1);
    }));
    assert_eq!(nodes[0].request_focus(), FocusRequestOutcome::Focused);
    deliveries.set(0);
    assert_eq!(nodes[1].request_focus(), FocusRequestOutcome::Focused);
    assert_eq!(nodes[0].request_focus(), FocusRequestOutcome::Focused);
    assert_eq!(
        deliveries.get(),
        2,
        "both focus transitions deliver to the listener"
    );
    c.bench_function(
        "delivery/FocusManager/two_attached_focus_transitions",
        |b| {
            b.iter(|| {
                black_box((
                    nodes[1].request_focus(),
                    nodes[0].request_focus(),
                    deliveries.get(),
                ))
            });
        },
    );
}

criterion_group!(
    delivery_paths,
    bench_registered_pointer_delivery,
    bench_cursor_changes,
    bench_focus_delivery,
);
criterion_main!(delivery_paths);
