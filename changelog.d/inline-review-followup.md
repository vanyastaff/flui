### Fixed

- Preserve finite extreme animation interpolation, cubic slopes near vertical tangents, exact angle half-turn ties and matrix NaN progress.
- Retain newer gesture contacts and IME attachments during reentrant cancellation and composition commit; cancel an owner-local post-frame tail when its owner retires.
- Rebind loading indicators without resetting their phase when their inherited Vsync changes, settle reached back-swipe destinations immediately, and revalidate paste authority after composition commit.
- Retire queued and active owner-local callbacks in registration order, preserving the earliest destructor failure.
- Contain native callback and diagnostic retirement failures, and preserve text-store backends for secondary headless windows.
- Publish live native scroll ranges and available directions, with activity and page physics shared by accessibility actions.
