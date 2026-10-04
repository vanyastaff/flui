### Fixed
- Friction simulations retain small motion and arrival times when drag approaches one, using cancellation-safe standard floating-point operations.
- Transform decomposition preserves finite large and tiny scales and reflected scales; layout diagonals avoid intermediate overflow and underflow.
