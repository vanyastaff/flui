//! `flui/serde` reaches every crate that owns a value type, not just the paint layer.
//!
//! The value types used to share one crate and one `serde` feature; they now live with their
//! owners, and the facade feature forwards to each. A value from an owner the forwarding
//! misses has no `Serialize` impl, so this file stops compiling.

use flui_foundation::geometry::{Offset, Rect};
use flui_interaction::Velocity;
use flui_objects::MainAxisSize;
use flui_painting::styling::Color;
use flui_platform_api::Locale;
use flui_rendering::constraints::AxisDirection;

fn round_trip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let json = serde_json::to_string(value).expect("the value serializes");
    serde_json::from_str(&json).expect("and deserializes")
}

#[test]
fn every_owner_crate_serializes_through_the_facade_feature() {
    let rect = Rect::from_ltrb(1.0, 2.0, 3.0, 4.0);
    assert_eq!(round_trip(&rect), rect);

    let color = Color::rgb(10, 20, 30);
    assert_eq!(round_trip(&color), color);

    let locale = Locale::new("en", Some("US"));
    assert_eq!(round_trip(&locale), locale);

    let velocity = Velocity::new(Offset::new(12.5, -3.0));
    assert_eq!(round_trip(&velocity), velocity);

    assert_eq!(
        round_trip(&AxisDirection::RightToLeft),
        AxisDirection::RightToLeft
    );
    assert_eq!(round_trip(&MainAxisSize::Min), MainAxisSize::Min);
}
