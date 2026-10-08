use flui_foundation::geometry::Offset;
use flui_interaction::{
    DragDownDetails, DragEndDetails, DragStartDetails, DragUpdateDetails, GestureEndReason,
    PointerKind, Velocity,
    recognizers::scale::{ScaleEndDetails, ScaleStartDetails, ScaleUpdateDetails},
};

fn main() {
    let _ = DragDownDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
    };
    let _ = DragStartDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        kind: PointerKind::Touch,
        timestamp: web_time::Instant::now(),
    };
    let _ = DragUpdateDetails {
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        delta: Offset::ZERO,
        primary_delta: 0.0,
        kind: PointerKind::Touch,
    };
    let _ = DragEndDetails {
        reason: GestureEndReason::Completed,
        velocity: Velocity::ZERO,
        global_position: Offset::ZERO,
        local_position: Offset::ZERO,
        primary_velocity: 0.0,
    };
    let _ = ScaleStartDetails {
        focal_point: Offset::ZERO,
        local_focal_point: Offset::ZERO,
        pointer_count: 2,
    };
    let _ = ScaleUpdateDetails {
        focal_point: Offset::ZERO,
        local_focal_point: Offset::ZERO,
        scale: 1.0,
        horizontal_scale: 1.0,
        vertical_scale: 1.0,
        rotation: 0.0,
        pointer_count: 2,
    };
    let _ = ScaleEndDetails {
        focal_point: Offset::ZERO,
        scale: 1.0,
        rotation: 0.0,
        velocity: 0.0,
    };
}
