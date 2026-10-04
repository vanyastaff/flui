### Fixed

- Keep the text-store conformance kit running after fixture panics whose opaque payloads contain panicking destructors, while preserving named failure diagnostics.
- Report a text-store panic before a supplied grant as a conformance failure instead of treating it as proof that the grant released its lock.
