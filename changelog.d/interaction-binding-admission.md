### Fixed
- Wheel and trackpad pinch claim handlers receive accepted signals even when a pointer listener panics; competing failures preserve the first panic and the next signal remains deliverable.
- Non-finite pointer Downs and contacts refused at the admission limit no longer dispatch their tails as hover. Refused identities use bounded storage; saturation suppresses untracked tails until lifecycle reset while admitted contacts and fresh valid Downs continue.
