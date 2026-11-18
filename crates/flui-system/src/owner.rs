//! What the crate's OS entries go through.

#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
pub(crate) mod panic_boundary;
