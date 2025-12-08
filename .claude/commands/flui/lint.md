---
name: Lint
description: Run cargo clippy with strict warnings on the workspace
---

!cargo clippy --workspace -- -D warnings 2>&1 | head -50

Analyze the clippy output above and:
1. Categorize issues by severity (errors, warnings)
2. Group by crate
3. Provide quick-fix suggestions for common issues

Focus on:
- Unused code
- Inefficient patterns
- Missing error handling
- Unsafe code usage
