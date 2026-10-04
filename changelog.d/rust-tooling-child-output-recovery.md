### Fixed
- Command output capture attempts to stop and reap its direct child when reading, writing or flushing fails, or the output sink unwinds, preserving the original failure. Refused termination does not introduce a blocking wait.
