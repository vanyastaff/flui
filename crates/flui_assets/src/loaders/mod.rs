//! Asset loaders for different sources.
//!
//! This module provides loaders for loading assets from various sources:
//! - [`FileLoader`] - Load assets from the file system
//! - [`BytesFileLoader`] - Load raw bytes from files
//! - [`MemoryLoader`] - Load assets from in-memory storage
//! - [`NetworkLoader`] - Load assets from HTTP/HTTPS (requires `network` feature)

pub mod file;
pub mod memory;
pub mod network;

pub use file::{BytesFileLoader, FileLoader};
pub use memory::MemoryLoader;
pub use network::NetworkLoader;



