### Changed

- An element's view-independent code (its build and update paths, lifecycle records and render
  object custody) is compiled once instead of once per view type (ADR-0405): in the generated
  counter the element code per view type falls from about 9.9 KB to 6 KB, and the stripped
  binary is 128 KB smaller. No authoring API changes.
