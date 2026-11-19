//! Text input: the input method capability a window offers
//! ([`PlatformTextInput`](crate::PlatformTextInput)), the composition events
//! a backend reports ([`ImeEvent`](crate::ImeEvent)), and the text store an
//! input method pulls from ([`text_store`], ADR-0090).

pub(crate) mod ime;
pub(crate) mod input;
pub mod text_store;
