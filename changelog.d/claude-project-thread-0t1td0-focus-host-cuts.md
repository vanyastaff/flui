### Changed

- A build pass keeps one buffer for the inherited dependencies its elements record, instead of allocating one per element that reads an inherited value.
- A hosted focus node's attachment lives in the allocation its host already shares with the node's listener, so mounting `Focus` or a control allocates once less.
