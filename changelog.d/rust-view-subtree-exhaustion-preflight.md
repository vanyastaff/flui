### Fixed

- Preflight generation exhaustion across every element being finalized before subtree removal detaches or tears down any node, preserving the intact subtree when removal refuses. Keyed subtrees retained for reparenting remain exempt from generation advancement.
