### Security

- `flui-desktop-mcp` resolves `rmcp` 3.9.0, which fixes RUSTSEC-2026-0457 (the OAuth client's SSRF filter checked host strings, not resolved addresses) and RUSTSEC-2026-0462 (unbounded response buffering in the Streamable HTTP clients).
