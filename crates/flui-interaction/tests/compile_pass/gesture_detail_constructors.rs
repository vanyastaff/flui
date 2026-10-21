use flui_foundation::geometry::Offset;
use flui_interaction::{
    DragUpdateDetails, ForcePressDetails, GestureArena, LongPressEndDetails,
    LongPressDetails, LongPressDownDetails, LongPressStartDetails, TapDetails,
    LongPressMoveUpdateDetails, PointerKind, TapDownDetails, TapGestureRecognizer, TapUpDetails,
    Velocity,
    gesture::{
        DoubleTapDetails, DoubleTapGestureRecognizer, LongPressGestureRecognizer,
        MultiTapDetails, MultiTapGestureRecognizer,
    },
};

fn main() {
    let global = Offset::new(40.0, 60.0);
    let local = Offset::new(10.0, 20.0);
    let pointer = flui_interaction::PointerId::try_from(1_u64).expect("authored contact");
    let _down = TapDownDetails::new(global, local, pointer).with_kind(PointerKind::Mouse);
    let _up = TapUpDetails::new(global, local, pointer).with_kind(PointerKind::Mouse);
    let _move = LongPressMoveUpdateDetails::new(global, local, Offset::ZERO, Offset::ZERO);
    let _end = LongPressEndDetails::new(global, local, Velocity::ZERO);
    let _pressure = ForcePressDetails::new(global, local, 0.5, 1.0);
    let _drag = DragUpdateDetails::new(global, local, Offset::ZERO, Offset::ZERO, 0.0, PointerKind::Mouse);

    let arena = GestureArena::new();
    let _tap = TapGestureRecognizer::builder(arena.clone())
        .on_tap_down(|TapDownDetails { global_position, .. }| { let _ = global_position; })
        .on_tap_move(|TapDetails { local_position, .. }| { let _ = local_position; })
        .on_tap_up(|TapUpDetails { global_position, .. }| { let _ = global_position; })
        .on_tap(|TapUpDetails { global_position, .. }| { let _ = global_position; })
        .on_tap_cancel(|TapDetails { kind, .. }| { let _ = kind; })
        .on_secondary_tap_down(|_: TapDownDetails| {})
        .on_secondary_tap_up(|_: TapUpDetails| {})
        .on_secondary_tap(|_: TapUpDetails| {})
        .on_tertiary_tap_down(|_: TapDownDetails| {})
        .on_tertiary_tap_up(|_: TapUpDetails| {})
        .on_tertiary_tap(|_: TapUpDetails| {})
        .build();
    let _double = DoubleTapGestureRecognizer::builder(arena.clone())
        .on_double_tap(|DoubleTapDetails { local_position, .. }| { let _ = local_position; })
        .build();
    let _long = LongPressGestureRecognizer::builder(arena.clone())
        .on_long_press_down(|LongPressDownDetails { global_position, .. }| { let _ = global_position; })
        .on_long_press_start(|LongPressStartDetails { local_position, .. }| { let _ = local_position; })
        .on_long_press_move_update(|LongPressMoveUpdateDetails { offset_from_origin, .. }| { let _ = offset_from_origin; })
        .on_long_press_up(|LongPressDetails { kind, .. }| { let _ = kind; })
        .on_long_press_end(|LongPressEndDetails { velocity, .. }| { let _ = velocity; })
        .on_long_press_cancel(|LongPressDetails { kind, .. }| { let _ = kind; })
        .build();
    let _multi = MultiTapGestureRecognizer::builder(arena, flui_interaction::MultiTapCount::TWO)
        .on_multi_tap(|MultiTapDetails { positions, .. }| { let _ = positions; })
        .build();
}
