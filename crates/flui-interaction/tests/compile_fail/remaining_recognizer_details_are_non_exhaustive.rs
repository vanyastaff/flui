use flui_foundation::geometry::Offset;
use flui_interaction::{
    ForcePressDetails, LongPressEndDetails, LongPressMoveUpdateDetails, PointerId, PointerKind,
    TapDownDetails, TapUpDetails, Velocity,
    recognizers::{
        DoubleTapDetails,
        long_press::{LongPressDetails, LongPressDownDetails, LongPressStartDetails},
        multi_tap::MultiTapDetails,
        multidrag::{MultiDragEndDetails, MultiDragUpdateDetails},
        tap::TapDetails,
    },
};

fn main() {
    let _ = TapDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = DoubleTapDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = LongPressDownDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = LongPressStartDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = LongPressDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = MultiTapDetails {
        pointer_count: 1,
        positions: vec![Offset::ZERO],
        center: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = MultiDragUpdateDetails {
        pointer_id: PointerId::try_from(1_u64).expect("authored contact"),
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        delta: Offset::ZERO,
        kind: PointerKind::Touch,
        timestamp: web_time::Instant::now(),
    };
    let _ = MultiDragEndDetails {
        pointer_id: PointerId::try_from(1_u64).expect("authored contact"),
        global_position: Offset::ZERO,
        velocity: Velocity::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = TapDownDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = TapUpDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = LongPressMoveUpdateDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        offset_from_origin: Offset::ZERO,
        local_offset_from_origin: Offset::ZERO,
    };
    let _ = LongPressEndDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        velocity: Velocity::ZERO,
    };
    let _ = ForcePressDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        pressure: 0.5,
        max_pressure: 1.0,
    };
}
