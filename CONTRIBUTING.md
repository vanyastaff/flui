# Contributing to FLUI

FLUI's full contributor guide lives in [`docs/contributing.md`](docs/contributing.md).

Before opening a pull request, run the local gate:

```bash
just ci
```

Rust code must follow the workspace-wide engineering standards, injected by the
Rust Code Studio plugin when you edit a matching file. Accepted architecture
decisions may impose stricter rules for a subsystem.

If `just` is not installed, run the equivalent commands listed in
[`docs/testing.md`](docs/testing.md). Render, layout, paint, lifecycle, and
reconciliation changes must also be checked against the Flutter reference per
[`docs/PORT.md`](docs/PORT.md).
