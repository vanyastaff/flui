### Changed

- AppKit tab joins accept a borrowed host window instead of a numeric ID, retain both native windows through the join, and return `false` for incompatible owners, closed windows or a self join.
