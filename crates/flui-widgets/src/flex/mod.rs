//! Flex widgets — lay out a sequence of children along a horizontal or
//! vertical axis ([`Row`] / [`Column`] / [`Flex`]) over `flui-objects`'
//! `RenderFlex`.

mod flex;
mod flexible;
mod spacer;

pub use flex::{Column, Flex, Row};
pub use flexible::{Expanded, Flexible};
pub use spacer::Spacer;
