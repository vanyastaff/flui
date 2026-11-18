//! # FLUI Foundation
//!
//! This crate provides fundamental types, utilities, and abstractions used
//! throughout the FLUI UI framework ecosystem. It contains zero-dependency or
//! minimal-dependency building blocks that other FLUI crates depend on.
//!
//! ## Overview
//!
//! FLUI Foundation contains:
//! - **Tree IDs**: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`
//!   for the 5-tree architecture
//! - **Tree structure**: the compile-time arity markers ([`Leaf`], [`Single`],
//!   [`Variable`], …) and [`IndexedSlot`] for child reconciliation
//! - **Keys**: `Key`, `ValueKey`, `UniqueKey` for widget identity
//!   (ObjectKey/GlobalKey in flui-view)
//! - **Change Notification**: Observable patterns for reactive UI updates
//! - **Listener IDs**: Stable identifiers for change-notification listeners
//! - **Inspect**: typed, borrowed self-description of nodes (`inspect`)
//! - **Error Handling**: Standardized error types and utilities
//! - **Notification System**: Base abstractions for event bubbling
//!
//! ## Design Principles
//!
//! 1. **Minimal Dependencies**: Only essential external crates
//! 2. **Zero-Cost Abstractions**: Performance-critical paths have no overhead
//! 3. **Ownership**: Notifications stay on their UI owner; values and IO
//!    capabilities declare their own thread bounds
//! 4. **Composability**: Types work well together and with external code
//! 5. **Stability**: Strong backwards compatibility guarantees
//!
//! ## Quick Start
//!
//! ```rust
//! use std::rc::Rc;
//!
//! use flui_foundation::{ChangeNotifier, ElementId, Listenable, UniqueKey};
//!
//! // Create unique identifiers
//! let element_id = ElementId::new(1);
//! let key = UniqueKey::new();
//!
//! // Observable values for reactive UI
//! let notifier = ChangeNotifier::new();
//! let subscription = notifier.listen(Rc::new(|| {
//!     // react to the change (e.g. mark a widget dirty)
//! }));
//!
//! // Notify listeners of changes
//! notifier.notify_listeners();
//! ```
//!
//! ## Feature Flags
//!
//! - `serde`: Enables serialization support for foundation types
//! - `creation-site`: Keeps creation sites (`CreationSite`) in release builds
//! - `testing`: Test support (`testing::CountingWaker`, `testing::panicking_waker`)
//!
//! ## Architecture
//!
//! Foundation types are designed to be:
//! - **Lightweight**: Minimal memory footprint
//! - **Fast**: Optimized for common operations
//! - **Safe**: Extensive use of type system for correctness
//! - **Debuggable**: Rich debugging and diagnostic information
//!
//! ## Examples
//!
//! ### Element Identification
//!
//! ```rust
//! use flui_foundation::{ElementId, UniqueKey, ValueKey, ViewKey};
//!
//! // Generational element identifier: packs slab index + generation into 8 bytes.
//! let element_id = ElementId::new(42); // 1-based: index() == 41
//! assert_eq!(element_id.index(), 41);
//! // Option<ElementId> has the same size (niche optimisation, generation >= 1)
//! assert_eq!(
//!     std::mem::size_of::<ElementId>(),
//!     std::mem::size_of::<Option<ElementId>>(),
//! );
//!
//! // Keys for element matching during rebuilds
//! let unique = UniqueKey::new();
//! let header = ValueKey::new("header");
//! assert!(!unique.key_eq(&header));
//! ```
//!
//! ### Change Notification
//!
//! ```rust
//! use std::rc::Rc;
//!
//! use flui_foundation::{ChangeNotifier, Listenable, ValueNotifier};
//!
//! // Basic change notification
//! let notifier = ChangeNotifier::new();
//! let _listening = notifier.listen(Rc::new(|| {
//!     // react to the change
//! }));
//!
//! // Value-holding notifier
//! let mut value_notifier = ValueNotifier::new(42);
//! let _value_listening = value_notifier.listen(Rc::new(|| {
//!     // react to the value change
//! }));
//! value_notifier.set_value(100);
//! assert_eq!(*value_notifier.value(), 100);
//! ```
//!
//! ## Thread Safety
//!
//! Values and notification channels have distinct thread boundaries:
//! - `ElementId`: `Send + Sync` (copy type)
//! - `Key`: `Send + Sync` (copy type)
//! - `ChangeNotifier`: owner-local callbacks and shared `Rc` listener storage
//! - `AtomicElementFlags`: Lock-free atomic operations
//!
//! ## Performance
//!
//! Foundation types are optimized for common UI patterns:
//! - `ElementId` uses a packed `NonZeroU64` (index + generation) for niche optimization
//! - Keys use atomic counters for O(1) generation
//! - Change notifiers use efficient listener storage
//! - Atomic flags provide lock-free state management

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Every public item is documented; keep it that way.
#![deny(missing_docs)]
// A library that prints owns a terminal it was never given; report through
// `tracing` or a returned error instead.
#![warn(clippy::print_stdout, clippy::print_stderr)]
#![warn(
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::pedantic
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Core modules - fundamental types with minimal dependencies
// Derive expansions use the same absolute owner path in library and integration targets.
#[allow(
    unused_extern_crates,
    reason = "derive expansions resolve the owner by its absolute crate name"
)]
extern crate self as flui_foundation;

#[cfg(test)]
mod test_cases;

mod affinity;
// The application identifier every platform names the application by.
mod app_id;
#[expect(
    unsafe_code,
    reason = "ADR-0328: the frame arena implements `Allocator`, an unsafe trait"
)]
pub mod arena;
// Compile-time child-count markers the render protocol attaches to nodes.
mod arity;
// Whether a build is running, for controllers below the signal graph (ADR-0086 §7).
mod build_phase;
// Generic at-most-once request/reply primitive (ADR-0039 §3): the
// owner lanes' claim-slot reply protocol composes this; its state-machine
// tests live here so CI actually runs them (same rationale as `affinity`).
mod claim;
mod clock;
// Monotonic generation/version counters for the window-runtime protocol:
// FrameEpoch, SurfaceGeneration.
mod epoch;
// The frame-identity group (address/epoch/surface_generation) bundled for
// the raster boundary — see the module doc for why it exists as its own
// builder-constructed type instead of positional constructor arguments.
mod frame_stamp;
// A slot's typed generation, and the one generation bookkeeping (`GenSlots`)
// under every packed generational key: fresh and recycled mints, the
// release's peek (before user code) and commit (after the release's own
// mutation), and a slot closed, forever, once its generation space ends.
pub mod generation;
// Geometry values: points, offsets, sizes, rectangles, insets, radii, matrices (ADR-0098).
pub mod geometry;
mod id;
mod key;
pub mod memory;
mod text;
// Input sequence identity and its owner-local mint (ADR-0207 §4)
mod sequence;
// Tickets that match an asynchronous answer to its request (ADR-0287 §5).
mod ticket;
// Panic-payload text extraction and the `BUG:` internal-invariant
// classifier every `catch_unwind` boundary in the workspace shares. Not in
// the prelude: this is a narrow, deliberately-reached-for utility, not a
// commonly-imported type.
pub mod panic;
#[cfg(feature = "testing")]
#[cfg_attr(docsrs, doc(cfg(feature = "testing")))]
pub mod testing;
// A child's position during reconciliation (index plus previous sibling).
mod child_slot;
// Slot-addressed side tables: membership and position without hashing.
mod slot_table;

// Shared field-name vocabulary for structured tracing events. Foundation owns
// the names because every framework crate emits them; *installing* a subscriber
// to receive them is a composition-root decision and lives in `flui-log`.
pub mod diagnostics;

// Dependency-inverted tree observation (ADR-0040) + the typed rebuild
// causes its events carry.
pub mod observe;
mod rebuild_reason;

// Reactive programming - change notification and observables
mod notifier;
// The read side of the ui_runtime's signal graph (ADR-0085 §2): handles, errors and
// the `ReadScope` contract. The graph itself lives in `flui-view`. Not
// re-exported from the root: reached as `flui_foundation::read_scope::…`.
pub mod read_scope;

// Typed self-description of the trees (ADR-0242); replaces `debug`'s string properties.
pub mod inspect;

// Where a value was created (`#[track_caller]`), kept in debug builds or with `creation-site`.
mod creation_site;

// An error with the path to the node it happened at, attached by the tree's owner.
mod tree_error;

// One path per item: a private module's items are named at the crate root, a
// public module's items only through the module.
pub use affinity::OwnerAffinity;
pub use app_id::{APP_ID_MAX_LEN, AppId, AppIdError, executable_class};
// `Range`, `Never` and `Optional` share their names with std and core: name
// them through the crate (`flui_foundation::Range`), never by a glob.
pub use arity::{Arity, AtLeast, Exact, Leaf, Never, Optional, Range, Single, Variable};
pub use build_phase::{BuildPhase, BuildPhaseGuard, MutatedDuringBuild};
pub use child_slot::IndexedSlot;
pub use claim::{ClaimHandle, ClaimOutcome, ClaimSlot, claim_slot};
pub use clock::{ManualClock, MonotonicClock, SystemClock, event_epoch};
pub use creation_site::CreationSite;
pub use epoch::{FrameEpoch, SurfaceGeneration};
pub use frame_stamp::FrameStamp;
pub use id::{
    DataTransferId, ElementId, GenId, Id, Identifier, LayerId, Marker, PresentationAddress,
    PresentationId, RawId, RenderId, SemanticsId, TreeId, UiRuntimeId, ViewId, markers,
};
pub use key::{AsViewKey, SaltedKey, UniqueKey, ValueKey, ViewKey};
pub use notifier::{
    ChangeNotifier, Listenable, ListenerCallback, ListenerObserver, NotificationCallback, Notifier,
    NotifierCallback, RecoveryCallback, Subscription, ValueListenable, ValueNotifier,
};
pub use rebuild_reason::{RebuildReason, RebuildReasons};
pub use sequence::{
    AtomicIdMint, DragSessionId, FrameCallbackId, IdExhausted, IdMint, InputSequenceId, ListenerId,
    MintedId, PressId,
};
pub use slot_table::{SlotIndex, SlotKey, SlotSet};
pub use text::{
    TextDirection, TextScaleProfile, TextSize, TextSizeError, TextSizeRequest, TextSizingIntent,
};
pub use ticket::{Ticket, TicketSource};
pub use tree_error::{PathEntry, TREE_CONTEXT_DEPTH, TreeContext, TreeError};
