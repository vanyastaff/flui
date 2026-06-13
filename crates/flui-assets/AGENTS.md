# AGENTS.md — flui-assets

High-performance asset management with smart caching, type safety, and async I/O.

**Status:** Not in workspace `default-members`. Build explicitly with `cargo build -p flui-assets`.

## What lives here

- **`AssetRegistry`** — global registry with TinyLFU eviction via `moka` cache
- **`Asset<T>` trait** — generic, type-safe asset loading
- **`FontAsset`** — font loading (built-in)
- **`lasso` interning** — 4-byte interned keys for fast hashing (`Rodeo`/`RodeoReader`)
- **Arc-based handles** — cheap cloning with automatic cleanup via weak references

## Key constraints

- **`images` feature** — gates `dep:image` for PNG/JPEG/GIF loading
- **`network` feature** — gates `dep:reqwest` for HTTP/HTTPS asset loading
- **`full` feature** — enables both `images` and `network`
- **Async I/O** — built on `tokio` (fs + io-util). All loading is non-blocking.
- **Future features** (commented out): `hot-reload` (notify), `mmap-fonts` (memmap2), `parallel-decode` (rayon)
