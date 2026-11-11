### Changed

- `ShapedParagraph::describe_spans` returns `Vec<String>`, formatted when called: a shaping records the spans' values and no longer formats their snapshot lines each time it shapes.
