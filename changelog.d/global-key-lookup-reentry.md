### Fixed

- Allow scoped GlobalKey comparisons to inspect or change the same public scope without holding a scope borrow across user callbacks. Repeated comparison mutation refuses admission after one retry; matching release uses passive claim identity and preserves independent key retirement failures.
