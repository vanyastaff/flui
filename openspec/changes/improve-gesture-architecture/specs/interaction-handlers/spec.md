# interaction-handlers Specification Delta

## New Requirements

### Requirement: Eager Winner Pattern

The GestureArena SHALL support eager winner resolution for faster gesture disambiguation.

#### Scenario: First recognizer accepts wins immediately

- **GIVEN** a GestureArena with multiple competing recognizers
- **WHEN** one recognizer accepts the gesture while arena is still open
- **THEN** that recognizer SHALL be marked as eager winner
- **AND** when arena closes, eager winner SHALL win the arena

#### Scenario: Multiple accept calls before close

- **GIVEN** multiple recognizers calling accept
- **WHEN** the arena closes
- **THEN** the first eager winner SHALL win
- **AND** subsequent accepters SHALL be rejected

### Requirement: Hold/Release Mechanism

The GestureArena SHALL support delaying sweep operations for time-based gestures.

#### Scenario: Hold prevents sweep

- **GIVEN** a recognizer that calls arena.hold()
- **WHEN** pointer up event occurs
- **THEN** sweep SHALL be deferred
- **AND** pending_sweep flag SHALL be set

#### Scenario: Release triggers deferred sweep

- **GIVEN** an arena with pending sweep
- **WHEN** recognizer calls arena.release()
- **THEN** deferred sweep SHALL execute
- **AND** arena SHALL resolve normally

#### Scenario: Long press uses hold/release

- **GIVEN** a LongPressGestureRecognizer
- **WHEN** pointer down occurs
- **THEN** recognizer SHALL hold the arena
- **WHEN** long press timer fires
- **THEN** recognizer SHALL accept and release

### Requirement: GestureBinding Coordination

GestureBinding SHALL provide centralized pointer event handling.

#### Scenario: Hit test caching

- **GIVEN** a pointer down event
- **WHEN** GestureBinding handles the event
- **THEN** hit test result SHALL be cached
- **AND** subsequent move/up events SHALL use cached result

#### Scenario: Pointer lifecycle management

- **GIVEN** a pointer event stream (down, move, up)
- **WHEN** events flow through GestureBinding
- **THEN** down SHALL trigger hit test and close arena
- **AND** up SHALL trigger sweep and cleanup cache

### Requirement: Device-Specific Gesture Settings

GestureSettings SHALL provide device-aware configuration.

#### Scenario: Touch device settings

- **GIVEN** a touch pointer event
- **WHEN** GestureSettings.for_device(Touch) is called
- **THEN** touch_slop SHALL be 18.0 (larger for finger)
- **AND** settings SHALL be tuned for touch input

#### Scenario: Mouse device settings

- **GIVEN** a mouse pointer event
- **WHEN** GestureSettings.for_device(Mouse) is called
- **THEN** touch_slop SHALL be 1.0 (precise cursor)
- **AND** settings SHALL be tuned for mouse input

### Requirement: OneSequenceGestureRecognizer

OneSequenceGestureRecognizer SHALL track single pointer sequences.

#### Scenario: Start tracking pointer

- **GIVEN** a recognizer implementing OneSequenceGestureRecognizer
- **WHEN** pointer down is received
- **THEN** startTrackingPointer() SHALL be called
- **AND** recognizer SHALL add entry to arena

#### Scenario: Stop tracking pointer

- **GIVEN** a tracked pointer
- **WHEN** gesture completes or is rejected
- **THEN** stopTrackingPointer() SHALL be called
- **AND** pointer SHALL be removed from tracking

### Requirement: PrimaryPointerGestureRecognizer State Machine

PrimaryPointerGestureRecognizer SHALL implement formal state machine.

#### Scenario: State transitions

- **GIVEN** a recognizer in Ready state
- **WHEN** pointer down occurs within tolerance
- **THEN** state SHALL transition to Possible

- **GIVEN** a recognizer in Possible state
- **WHEN** arena is won
- **THEN** state SHALL transition to Accepted

- **GIVEN** a recognizer in Possible state
- **WHEN** pointer moves beyond slop tolerance
- **THEN** state SHALL transition to Defunct

#### Scenario: Deadline timer support

- **GIVEN** a recognizer with deadline configured
- **WHEN** deadline expires in Possible state
- **THEN** recognizer SHALL resolve (accept or reject)
- **AND** state SHALL transition accordingly

## Modified Requirements

### Requirement: Gesture Recognition (Modified)

The framework SHALL support multi-gesture recognition with conflict resolution using eager winner pattern.

#### Scenario: Resolve gesture conflicts (Modified)

- **GIVEN** multiple gestures competing for same pointer
- **WHEN** a gesture accepts the gesture
- **THEN** if arena open, gesture SHALL be marked as eager winner
- **WHEN** arena closes
- **THEN** eager winner SHALL win immediately
- **AND** other gestures SHALL be rejected
