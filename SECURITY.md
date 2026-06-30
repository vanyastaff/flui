# Security Policy

## Supported Versions

FLUI is pre-1.0 and not yet published to crates.io. Security fixes are made on
`main`; downstream users should track the latest commit until the first stable
release line exists.

## Reporting a Vulnerability

Do not open a public issue for a suspected vulnerability.

Use GitHub's private vulnerability reporting for this repository. Include:

- affected crate or binary;
- minimal reproduction steps;
- expected and actual impact;
- platform, Rust toolchain, and commit hash;
- whether the issue requires untrusted input, local filesystem access, GPU
  access, or a network path.

If private reporting is unavailable, open a public issue that says only that a
security report is available and do not include exploit details.

## Scope

Security-sensitive areas include platform backends, hot reload and dynamic
library loading, asset/network loading, GPU resource management, unsafe blocks,
and build tooling that executes external commands.

