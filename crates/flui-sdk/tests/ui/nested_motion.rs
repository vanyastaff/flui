use flui_sdk::animation::{AnimationVector, Lerp, TwoWayConverter};
use flui_sdk::geometry::Offset;
use flui_sdk::painting::Color;

type Position = Offset<f64>;

#[derive(Clone, TwoWayConverter)]
struct Appearance {
    position: Position,
    color: Color,
}

#[derive(Clone, TwoWayConverter)]
struct Motion<const TAG: usize>(Appearance, f64);

fn main() {
    let start = Motion::<7>(
        Appearance { position: Offset::ZERO, color: Color::rgb(255, 0, 0) },
        0.0,
    );
    let finish = Motion::<7>(
        Appearance { position: Offset::new(4.0, 8.0), color: Color::rgba(0, 0, 255, 0) },
        1.0,
    );
    assert_eq!(<Motion<7> as TwoWayConverter>::Vector::COMPONENTS, 7);
    let midpoint = start.lerp_to(&finish, 0.5);
    assert_eq!(midpoint.0.position, Offset::new(2.0, 4.0));
    assert_eq!(midpoint.0.color.r, 255);
    let restored = Motion::<7>::from_vector(midpoint.to_vector());
    assert_eq!(restored.0.position, midpoint.0.position);
    assert_eq!(restored.1, 0.5);
}
