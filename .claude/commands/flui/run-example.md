---
name: Run Example
description: Build and run a FLUI example with timeout protection
---

Run the specified FLUI example: **$ARGUMENTS**

Steps:
1. Build the example: `cargo build --example <example_name>`
2. Run with timeout: `timeout 10 cargo run --example <example_name>`
3. If it crashes or hangs, analyze the issue

Available examples can be found with: `ls examples/`

Default timeout is 10 seconds for interactive examples. Use Ctrl+C to stop.
