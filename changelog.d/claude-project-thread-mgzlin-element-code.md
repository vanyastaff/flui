### Changed

- An element's view-independent code (its panic recovery, lifecycle records and render object
  custody) is compiled once instead of once per view type (ADR-0405): in the generated counter
  the element code per view type falls from about 6.4 KB to 5.4 KB, and its machine code by
  21 KB. No authoring API changes.
