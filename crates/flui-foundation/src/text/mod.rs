//! Text values the layers below the text engine share: the direction text
//! flows in, and the sizes an author asks for.

mod direction;
mod sizing;

pub use direction::TextDirection;
pub use sizing::{TextScaleProfile, TextSize, TextSizeError, TextSizeRequest, TextSizingIntent};
