# Contributing to FLUI

FLUI's full contributor guide lives in [`docs/contributing.md`](docs/contributing.md).

Before opening a pull request, run the local gate:

```bash
just ci
```

Rust code must follow the workspace-wide engineering standards in
[`AGENTS.md`](AGENTS.md) and [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md).
Accepted architecture decisions (`docs/adr/`) may impose stricter rules for a
subsystem.

If `just` is not installed, run the equivalent commands listed in
[`docs/testing.md`](docs/testing.md). Render, layout, paint, lifecycle, and
reconciliation changes must also be checked against the Flutter reference per
[`docs/PORT.md`](docs/PORT.md).
