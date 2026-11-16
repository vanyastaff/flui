# ADR-0342: A variant or value added to a closed union breaks the agent protocol, and 0.13 is such a break

- **Status:** Proposed (2026-11-16)
- **Date:** 2026-11-16
- **Related:** [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (what an additive change
  is), [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §1 (the protocol version and its
  golden schemas), [ADR-0238](ADR-0238-flui-driver-protocol.md) §2 (events reach a connection by
  subscription), [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) §4 (the native
  recording whose new rows make 0.13 a break)

## Context and problem statement

`flui-protocol`'s rule (`version.rs`) calls a change additive when a reader built on an older
schema still reads the newer reply, and lists what is not: a field renamed or removed, a name
respelled, an optional field made required. The `wire_schema` test that enforces it saw
neither kind of growth a closed union can have. It named every array item `*`, so the required
fields of all variants of a `oneOf` merged, and it checked enumerated strings for loss only, so a
value added to an `enum` list or a `oneOf` of constants passed unseen. serde refuses a tag or a
value it does not know and fails the whole message: an older client that meets a new variant of
a union it reads unasked cannot read that message at all. The native recording rows `store_focus`,
`store_lock`, `close_request`, `closed` and `cut` reach a client that started a recording of the
`native` layer inside `record_native` events, which no older client can read. Which growth of a
union is additive, and what is 0.13?

## Decision drivers

- The rule is one source: `version.rs` states it, the `wire_schema` test implements exactly it.
- An app refuses only the clients a `BREAKING` version separates from it
  (`ProtocolVersion::serves`); a break the table does not list lets an older client in to fail.
- Published versions are served by apps already released; re-judging them changes nothing they
  do.
- Some unions are open by design: an unknown role, action name or error code has a fallback, and
  an event kind reaches only a client that asked for it by name.

## Considered options

1. A variant or value added to a closed union is a break, except in a union with a fallback or one
   met only by asking for it by name; 0.13 is listed in `BREAKING`.
2. Every new variant or value is additive, because an older reader refuses it by name.
3. Give every union a catch-all variant (`#[serde(other)]`) so that a new one always reads.

## Decision

Chosen option: 1, because it is what the rule's own definition implies, and the table that makes
apps refuse older clients only works if the test knows a break when it sees one.

§1. A variant added to a tagged union, or a value added to a closed string union (an `enum` list
or a `oneOf` of constants), is not additive. The `wire_schema` test names a tagged variant by its
tag (`row=store_lock`) and a union's values by their container, and reports `variant added` and
`value added`. They pass only in a version `BREAKING` lists.

§2. A union is open to a new value only by a decision recorded in the test beside its reason: a
fallback in the reader (`Role::Unknown`, an unknown action name dropped, `ErrorCode::Platform`),
or delivery only on request by name (the `Event` union and `EventKind`, ADR-0238 §2). Events sent
unasked (`gap`, `record`, `record_native`) are not open: what they carry is checked as any union.

§3. The versions published while the test could not see variants keep what they added, as a closed
list in the test with what each added (0.8: `InspectValue::NonFinite`; 0.9: `DeviceClass::Other`,
`HandleKind::Recording`; 0.10: `RecordLayer::Native`). No later version is added to it.

§4. Protocol 0.13 is a break: `BREAKING` lists it with this record. An app speaking 0.13 refuses
a client of 0.12 or older.

### Consequences

- Good, because a client never meets a row it cannot read: the app refuses it at hello.
- Good, because a later version that adds a close answer, a cut reason or a store error must say
  so in `BREAKING`, or the test fails.
- Bad, because each such addition costs older clients their connection until they update; the
  driver and the CLI build from the same tree, so in practice they move together.
- Neutral, because the published versions before 0.13 are not re-judged.

### Confirmation

`wire_schema::every_published_schema_is_additive_to_the_next` over every published schema, and
`the_additivity_check_refuses_a_removed_field_a_respelled_name_and_a_new_required_one`, whose
cases add a variant to a recording, a request and an event-shaped union and a value to a `oneOf`
of constants and to an `enum` list (each refused outside a breaking version), and a value to the
`Role` union and a kind to the `Event` union (each allowed).

## Pros and cons of the options

### 1. A break, with recorded open unions

- Good, because the rule and its test say the same thing.
- Bad, because every growth of a closed union is a protocol break.

### 2. Every new variant is additive

- Good, because no version breaks for a new row.
- Bad, because an older client then reads the hello, starts a recording and fails at the first new
  row, which the version negotiation existed to prevent.

### 3. A catch-all variant in every union

- Good, because every addition becomes additive.
- Bad, because a reader that reads a recording row as "unknown" cannot replay it: a recording's
  row must be refused by name, never skipped (ADR-0215 §2), so the catch-all would move the
  failure, not remove it.

## More information

The test lives in `crates/flui-protocol/tests/wire_schema.rs`; the rule in
`crates/flui-protocol/src/version.rs`. Revisit when the protocol reaches 1.0, where a break also
bumps `major` (ADR-0080).
