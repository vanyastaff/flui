//! The file store behind the `storage` feature: directories created on the
//! first write, values replaced atomically, inaccessible paths reported apart
//! from corrupt ones, and a write based on a stale version refused.
