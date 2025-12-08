---
name: Dependencies
description: Analyze crate dependencies and check for issues
---

!cargo tree --workspace --depth 2 2>&1 | head -100

Analyze the dependency tree:
1. Check for duplicate versions of the same crate
2. Identify heavy dependencies
3. Look for security advisories with `cargo audit` if available
4. Suggest optimization opportunities

Focus on the crate: **$ARGUMENTS** (if specified)
