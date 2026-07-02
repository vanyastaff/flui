# Contributing to FLUI

FLUI's full contributor guide lives in [`docs/contributing.md`](docs/contributing.md).

Before opening a pull request, run the local gate:

```bash
just ci
```

If `just` is not installed, run the equivalent commands listed in
[`docs/testing.md`](docs/testing.md). Render, layout, paint, lifecycle, and
reconciliation changes must also be checked against the Flutter reference per
[`docs/PORT.md`](docs/PORT.md).

