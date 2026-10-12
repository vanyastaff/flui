### Changed

- Animation phases `Completed` and `Dismissed` are now `ForwardSettled` and
  `ReverseSettled`, with `is_forward_settled()` and `is_reverse_settled()` helpers.
  Settlement does not imply endpoint arrival or successful run completion.
  Optional serde uses the new variant spellings; persisted phase values need migration.
- Replace `ConstantAnimation::completed` and `dismissed` with
  `ConstantAnimation::with_status` and `AnimationStatus::ForwardSettled` or
  `AnimationStatus::ReverseSettled`. Replace `ALWAYS_COMPLETE` and `ALWAYS_DISMISSED` with the
  constants `UNIT_END_ANIMATION` and `UNIT_START_ANIMATION` for fixed values one
  and zero in the normalized interval.
