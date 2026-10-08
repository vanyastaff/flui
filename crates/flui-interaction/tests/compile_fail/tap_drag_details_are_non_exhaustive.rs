use flui_foundation::geometry::Offset;
use flui_interaction::{
    PointerKind, TapDragDownDetails, TapDragEndDetails, TapDragStartDetails, TapDragUpDetails,
    TapDragUpdateDetails, Velocity,
};

fn main() {
    let _ = TapDragDownDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
        consecutive_tap_count: 1,
    };
    let _ = TapDragUpDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
        consecutive_tap_count: 1,
    };
    let _ = TapDragStartDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
        consecutive_tap_count: 1,
    };
    let _ = TapDragUpdateDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        delta: Offset::ZERO,
        kind: PointerKind::Touch,
        consecutive_tap_count: 1,
    };
    let _ = TapDragEndDetails {
        velocity: Velocity::ZERO,
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        consecutive_tap_count: 1,
    };
}
