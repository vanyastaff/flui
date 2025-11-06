//! Basic single-child layout widgets.
//!
//! This module contains fundamental widgets for basic layout operations:
//! - Container: A convenience widget combining sizing, padding, and decoration
//! - SizedBox: A box with fixed dimensions
//! - Padding: Insets its child by padding
//! - Center: Centers its child
//! - Align: Aligns its child with flexible positioning

pub mod align;
pub mod aspect_ratio;
pub mod builder;
pub mod button;
pub mod card;
pub mod center;
pub mod colored_box;
pub mod constrained_box;
pub mod container;
pub mod decorated_box;
pub mod divider;
pub mod fitted_box;
pub mod limited_box;
pub mod padding;
pub mod safe_area;
pub mod sized_box;
pub mod text;
pub mod vertical_divider;



// Re-exports
pub use align::Align;
pub use aspect_ratio::AspectRatio;
pub use builder::Builder;
pub use button::Button;
pub use card::Card;
pub use center::Center;
pub use colored_box::ColoredBox;
pub use constrained_box::ConstrainedBox;
pub use container::Container;
pub use decorated_box::DecoratedBox;
pub use divider::Divider;
pub use fitted_box::FittedBox;
pub use limited_box::LimitedBox;
pub use padding::Padding;
pub use safe_area::SafeArea;
pub use sized_box::SizedBox;
pub use text::Text;
pub use vertical_divider::VerticalDivider;


