### Removed

- Removed the unused `DiagnosticsNode::to_json` exporter, whose hand-written string escaping could produce invalid JSON. Diagnostics values retain serialization through the existing `serde` feature; agent transports use the typed protocol schema.
