### Fixed

- Animation registry owner retirement stores known cleanup custody directly in the controller delivery queue, avoiding a boxed callback allocation while preserving reentrant delivery and first-failure cleanup ordering.
