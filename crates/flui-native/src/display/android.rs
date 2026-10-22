//! The Android displays: the `Display`s the system lists and the Activity's own,
//! read over JNI with the choreographer's refresh rate ([`source`]), and what a
//! sample of a `Display` becomes with no JVM ([`sample`]).

pub(crate) mod sample;
#[cfg(target_os = "android")]
pub(crate) mod source;

#[cfg(target_os = "android")]
pub(crate) use source::AndroidDisplays;
