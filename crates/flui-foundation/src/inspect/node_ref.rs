//! A reference to a node that names its window, for ids that cross windows (ADR-0238).
//!
//! A [`Link`] belongs to the tree that holds it: `r12` means nothing without the window whose
//! render tree it is in. A [`NodeRef`] adds that window, printed `p3/r12`, so an id read from one
//! window and sent to another is recognised for what it is instead of naming an unrelated node.

use core::fmt;
use core::num::{NonZeroU32, NonZeroU64};
use core::str::FromStr;

use crate::id::{ElementId, LayerId, PresentationId, RenderId, SemanticsId};

use super::tree::{Link, SceneLayer};

/// A node of one of a window's trees, with the window.
///
/// Its text form is the presentation's slot (`p3`, with `.2` for a generation past the first,
/// as a [`Link`] does) and the node's link: `p3/v12.4`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeRef {
    /// The window.
    pub presentation: PresentationId,
    /// The node, in that window's trees.
    pub link: Link,
}

impl NodeRef {
    /// `link` in `presentation`'s trees.
    #[must_use]
    pub const fn new(presentation: PresentationId, link: Link) -> Self {
        Self { presentation, link }
    }
}

impl fmt::Display for NodeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{}", self.presentation.index())?;
        let generation = self.presentation.generation().get();
        if generation > 1 {
            write!(f, ".{generation}")?;
        }
        write!(f, "/{}", self.link)
    }
}

/// Text that is not a [`Link`] or a [`NodeRef`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{text}` is not a node reference")]
pub struct ParseRefError {
    text: String,
}

impl ParseRefError {
    fn of(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }
}

/// A slot and the generation after the dot, when there is one: `12.4`, `12`.
fn slot_and_generation(text: &str) -> Option<(u64, Option<NonZeroU64>)> {
    let (slot, generation) = match text.split_once('.') {
        Some((slot, generation)) => (slot, Some(generation)),
        None => (text, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(slot) || generation.is_some_and(|generation| !digits(generation)) {
        return None;
    }
    let generation = match generation {
        // A first generation prints no suffix, so a written one is another spelling of the same
        // node: refused, so every node has one text.
        Some(generation) => {
            Some(NonZeroU64::new(generation.parse::<u64>().ok()?).filter(|g| g.get() > 1)?)
        }
        None => None,
    };
    Some((slot.parse::<u64>().ok()?, generation))
}

impl FromStr for Link {
    type Err = ParseRefError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || ParseRefError::of(text);
        let prefix = text.chars().next().ok_or_else(error)?;
        let (slot, generation) =
            slot_and_generation(&text[prefix.len_utf8()..]).ok_or_else(error)?;
        let generation32 = || -> Result<NonZeroU32, ParseRefError> {
            let generation = generation.map_or(1, NonZeroU64::get);
            NonZeroU32::new(u32::try_from(generation).map_err(|_| error())?).ok_or_else(error)
        };
        Ok(match prefix {
            'v' => Self::Element(ElementId::new_gen(
                u32::try_from(slot).map_err(|_| error())?,
                generation32()?,
            )),
            'r' => Self::Render(RenderId::new_gen(
                u32::try_from(slot).map_err(|_| error())?,
                generation32()?,
            )),
            'l' => Self::Layer(SceneLayer {
                scene: generation.unwrap_or(NonZeroU64::MIN),
                id: LayerId::new_checked(usize::try_from(slot).map_err(|_| error())?)
                    .ok_or_else(error)?,
            }),
            's' if generation.is_none() => Self::Semantics(
                SemanticsId::new_checked(usize::try_from(slot).map_err(|_| error())?)
                    .ok_or_else(error)?,
            ),
            _ => return Err(error()),
        })
    }
}

impl FromStr for NodeRef {
    type Err = ParseRefError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || ParseRefError::of(text);
        let (window, link) = text.split_once('/').ok_or_else(error)?;
        let window = window.strip_prefix('p').ok_or_else(error)?;
        let (slot, generation) = slot_and_generation(window).ok_or_else(error)?;
        let generation = generation.map_or(1, NonZeroU64::get);
        let presentation = PresentationId::new_gen(
            u32::try_from(slot).map_err(|_| error())?,
            NonZeroU32::new(u32::try_from(generation).map_err(|_| error())?).ok_or_else(error)?,
        );
        Ok(Self {
            presentation,
            link: link.parse::<Link>()?,
        })
    }
}
