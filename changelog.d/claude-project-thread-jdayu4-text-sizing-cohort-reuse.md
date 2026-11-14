### Added

- `TextSizingCohort::retire` and `TextSizing::begin_cohort_in`: a finished text sizing attempt lends its allocation to the next one, and policies of the retired attempt retain nothing in it. The runtime uses them, so a steady frame begins its text sizing attempt without allocating.
