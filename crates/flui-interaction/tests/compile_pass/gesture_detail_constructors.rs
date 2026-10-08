use flui_foundation::geometry::Offset;
use flui_interaction::{
    DragUpdateDetails, ForcePressDetails, GestureArena, LongPressEndDetails,
    LongPressMoveUpdateDetails, PointerKind, TapDownDetails, TapGestureRecognizer, TapUpDetails,
    Velocity,
    recognizers::{
        DoubleTapDetails, DoubleTapGestureRecognizer, LongPressGestureRecognizer,
        MultiTapGestureRecognizer,
        long_press::{LongPressDetails, LongPressDownDetails, LongPressStartDetails},
        multi_tap::MultiTapDetails,
        tap::TapDetails,
    },
};

fn main() {
    let global = Offset::new(40.0, 60.0);
    let local = Offset::new(10.0, 20.0);
    let _down = TapDownDetails::new(global, local).with_kind(PointerKind::Mouse);
    let _up = TapUpDetails::new(global, local).with_kind(PointerKind::Mouse);
    let _move = LongPressMoveUpdateDetails::new(global, local, Offset::ZERO, Offset::ZERO);
    let _end = LongPressEndDetails::new(global, local, Velocity::ZERO);
    let _pressure = ForcePressDetails::new(global, local, 0.5, 1.0);
    let _drag = DragUpdateDetails::new(global, local, Offset::ZERO, 0.0, PointerKind::Mouse);

    let arena = GestureArena::new();
    let _tap = TapGestureRecognizer::builder(arena.clone())
        .on_tap(|TapDetails { global_position, .. }| { let _ = global_position; })
        .build();
    let _double = DoubleTapGestureRecognizer::builder(arena.clone())
        .on_double_tap(|DoubleTapDetails { local_position, .. }| { let _ = local_position; })
        .build();
    let _long = LongPressGestureRecognizer::builder(arena.clone())
        .on_long_press_down(|LongPressDownDetails { global_position, .. }| { let _ = global_position; })
        .on_long_press_start(|LongPressStartDetails { local_position, .. }| { let _ = local_position; })
        .on_long_press_end(|LongPressDetails { kind, .. }| { let _ = kind; })
        .build();
    let _multi = MultiTapGestureRecognizer::builder(arena, 2)
        .on_multi_tap(|MultiTapDetails { positions, .. }| { let _ = positions; })
        .build();
}
