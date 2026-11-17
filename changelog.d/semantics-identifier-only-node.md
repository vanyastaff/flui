### Fixed

- `find::id` and the agent find a `Semantics` container named only by its identifier. The agent
  left it out as a generic container, as the OS adapters do, so a widget that publishes no
  semantics of its own could not be selected by the identifier its author gave it. The tree the
  OS adapters receive is unchanged.
