# Testing

[`tests/agent_workflow.rs`](https://github.com/vanyastaff/flui/blob/main/tests/agent_workflow.rs)
is the canonical headless test recipe: mount the counter tree, dump the render diagnostics, find
the button by its accessible label, tap its bounds through pointer replay, and assert the
rendered count advanced — using only `flui::…`, no window. A second test in the same file shows
the actionable failure a missing accessibility label produces.

```bash
cargo test --test agent_workflow
```

[`docs/testing.md`](https://github.com/vanyastaff/flui/blob/main/docs/testing.md) walks through
the same five steps in prose, and is the map of the whole test pyramid (unit → headless frame
tests → the render-object harness catalog → live E2E smoke) if you need a tier below or above
this one for what you're testing.
