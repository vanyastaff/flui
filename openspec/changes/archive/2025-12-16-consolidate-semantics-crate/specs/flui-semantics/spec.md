# Specification: flui-semantics

The flui-semantics crate provides the accessibility/semantics tree for FLUI applications.

## ADDED Requirements

### Requirement: Semantics Node Storage
The system SHALL provide a `SemanticsNode` type that stores accessibility information for a single UI element.

#### Scenario: Create node with label
- **WHEN** creating a SemanticsNode with label "Submit Button"
- **THEN** the node SHALL store the label as SmolStr
- **AND** `node.label()` SHALL return `Some("Submit Button")`

#### Scenario: Node with children uses SmallVec
- **WHEN** adding up to 4 children to a SemanticsNode
- **THEN** children SHALL be stored inline without heap allocation
- **AND** `node.children()` SHALL return all added child IDs

### Requirement: Semantics Configuration
The system SHALL provide a `SemanticsConfiguration` type for building node properties.

#### Scenario: Configure button semantics
- **WHEN** configuring a node as a button with tap action
- **THEN** `config.set_button(true)` SHALL set the IsButton flag
- **AND** `config.add_action(SemanticsAction::Tap, handler)` SHALL register the handler
- **AND** `config.is_button()` SHALL return true

#### Scenario: Configuration absorb merges properties
- **WHEN** calling `parent_config.absorb(&child_config)`
- **THEN** parent SHALL merge child's flags, actions, and labels
- **AND** parent's existing values SHALL take precedence

### Requirement: Semantics Actions
The system SHALL provide `SemanticsAction` enum with all standard accessibility actions.

#### Scenario: Action bitmask encoding
- **WHEN** encoding actions as bitmask
- **THEN** `SemanticsAction::Tap.value()` SHALL return `1 << 0`
- **AND** `SemanticsAction::LongPress.value()` SHALL return `1 << 1`
- **AND** actions SHALL be combinable via bitwise OR

#### Scenario: Action handler invocation
- **WHEN** performing an action on a node
- **THEN** the registered `SemanticsActionHandler` SHALL be called
- **AND** handler SHALL receive action type and optional `ActionArgs`

### Requirement: Semantics Flags
The system SHALL provide `SemanticsFlag` enum and `SemanticsFlags` bitset for boolean properties.

#### Scenario: Flag operations
- **WHEN** setting and clearing flags
- **THEN** `flags.set(SemanticsFlag::IsButton)` SHALL enable the flag
- **AND** `flags.has(SemanticsFlag::IsButton)` SHALL return true
- **AND** `flags.clear(SemanticsFlag::IsButton)` SHALL disable the flag

### Requirement: Semantics Tree Management
The system SHALL provide `SemanticsOwner` for managing the semantics tree lifecycle.

#### Scenario: Create and track nodes
- **WHEN** creating nodes via SemanticsOwner
- **THEN** `owner.create_node_with_new_id()` SHALL allocate unique IDs
- **AND** `owner.node(id)` SHALL return the created node
- **AND** `owner.node_count()` SHALL reflect total nodes

#### Scenario: Dirty tracking for updates
- **WHEN** modifying a node's properties
- **THEN** `owner.has_dirty_nodes()` SHALL return true
- **AND** `owner.build_update()` SHALL include modified nodes
- **AND** after build, dirty flags SHALL be cleared

#### Scenario: Node removal cascades to descendants
- **WHEN** removing a node with children
- **THEN** all descendant nodes SHALL also be removed
- **AND** removed IDs SHALL appear in `SemanticsUpdate.removed_node_ids`

### Requirement: Semantics Events
The system SHALL provide `SemanticsEvent` for accessibility notifications.

#### Scenario: Announce message to assistive technology
- **WHEN** creating `SemanticsEvent::announce("Item selected")`
- **THEN** event type SHALL be `SemanticsEventType::Announce`
- **AND** `event.get_string("message")` SHALL return "Item selected"

#### Scenario: Focus event with node ID
- **WHEN** creating `SemanticsEvent::focus(42)`
- **THEN** event type SHALL be `SemanticsEventType::Focus`
- **AND** `event.get_int("nodeId")` SHALL return 42

### Requirement: Semantics Update Protocol
The system SHALL provide `SemanticsUpdate` for batched platform updates.

#### Scenario: Build update from dirty nodes
- **WHEN** calling `owner.build_update()` with dirty nodes
- **THEN** update SHALL contain `SemanticsNodeData` for each dirty node
- **AND** update SHALL contain IDs of removed nodes
- **AND** `update.is_empty()` SHALL return false

### Requirement: String Optimization with SmolStr
The system SHALL use `SmolStr` for all text fields to optimize memory and cloning.

#### Scenario: Small string inline storage
- **WHEN** setting label to string shorter than 24 bytes
- **THEN** string SHALL be stored inline without heap allocation
- **AND** `clone()` SHALL complete in O(1) time

#### Scenario: SmolStr API compatibility
- **WHEN** using SmolStr fields
- **THEN** `label.as_str()` SHALL return `&str`
- **AND** `&label` SHALL auto-deref to `&str`
- **AND** `SmolStr::from("text")` SHALL work for construction

### Requirement: Collection Optimization with SmallVec
The system SHALL use `SmallVec` for children and actions collections.

#### Scenario: Inline storage for small collections
- **WHEN** node has 4 or fewer children
- **THEN** children SHALL be stored inline on stack
- **AND** no heap allocation SHALL occur

#### Scenario: Graceful overflow to heap
- **WHEN** node has more than 4 children
- **THEN** SmallVec SHALL transparently allocate on heap
- **AND** all children SHALL remain accessible

### Requirement: AccessKit Platform Integration
The system SHALL provide optional AccessKit integration for platform accessibility APIs.

#### Scenario: Convert node to AccessKit format
- **WHEN** calling `node.to_accesskit()` (with accesskit feature)
- **THEN** result SHALL be valid `accesskit::Node`
- **AND** label SHALL map to AccessKit name
- **AND** IsButton flag SHALL map to `Role::Button`

#### Scenario: Map FLUI actions to AccessKit actions
- **WHEN** node has `SemanticsAction::Tap`
- **THEN** AccessKit node SHALL have `Action::Click`
- **AND** action handler SHALL be invokable via AccessKit

### Requirement: Hit Testing
The system SHALL provide hit testing for accessibility focus.

#### Scenario: Hit test finds deepest node
- **WHEN** calling `owner.hit_test(position)`
- **THEN** result SHALL be the deepest node containing position
- **AND** hidden nodes SHALL be excluded
- **AND** nodes without actions SHALL bubble to parent

### Requirement: Consistent ID Pattern
The system SHALL use `SemanticsId` consistent with other FLUI ID types.

#### Scenario: NonZeroUsize optimization
- **WHEN** using `Option<SemanticsId>`
- **THEN** size SHALL be 8 bytes (same as SemanticsId)
- **AND** None SHALL be represented without extra storage

#### Scenario: Slab index conversion
- **WHEN** storing node at slab index 5
- **THEN** `SemanticsId::new(6)` SHALL be used (index + 1)
- **AND** `id.get() - 1` SHALL return original slab index
