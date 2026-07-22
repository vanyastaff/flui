//! Stack widgets — overlap children in a single box ([`Stack`],
//! [`IndexedStack`]) over `flui-objects`' stack render objects.

mod positioned;
mod stack;

pub use positioned::Positioned;
pub use stack::{IndexedStack, Stack};
