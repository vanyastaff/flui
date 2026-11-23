### Fixed

- `MergeSemantics` over a scroll view no longer folds the scroll position's slider into the
  merged `ScrollView`, which published a numeric range ARIA does not give that role: the slider
  stays the scroll view's child node, as it is outside a merge.
