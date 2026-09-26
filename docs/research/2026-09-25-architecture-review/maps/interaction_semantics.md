# Codebase map: interaction_semantics (flui-interaction, flui-semantics, and their seams to flui-view / flui-widgets / flui-app / flui-platform / tools/desktop-mcp)

_Raw output of the `map:interaction_semantics` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

flui-interaction (layer 2, about 51k lines including inline tests) is a near file-for-file port of Flutter's gestures package: an owner-local GestureArena, 11+ recognizers, velocity/resampling, a per-presentation FocusManager/FocusScopeNode with reading-order traversal and bubbling key dispatch, MouseTracker, HitTestResult, and TextInputOwner, the token-guarded IME connection that owns a flui-platform PlatformTextInput. It also holds InteractionLane, a registry of owner-local closures kept in a thread_local. Render objects must be Send + Sync (flui-view RenderView::RenderObject), so they store opaque target IDs (ScrollTarget, PathClipTarget, ShaderMaskTarget). Those IDs are resolved on the owner thread through the ambient ACTIVE_LANES TLS. Event data uses the W3C ui-events vocabulary shared with flui-platform. Shortcuts, Actions and Intent (keyed by TypeId) and the Focus widgets live in flui-widgets/src/interaction.

flui-semantics (layer 3, about 11k lines) defines a model shaped like Flutter's: a SemanticsConfiguration with a SemanticsFlag bitset, a SemanticsAction bitmask, an optional SemanticsRole, and a slab SemanticsTree. SemanticsOwner lives as Option<SemanticsOwner> on flui-rendering's PipelineOwner. It is created only while semantics is enabled. Assembly is mark-scoped in rendering's post-paint phase (flui-rendering/src/pipeline/owner/semantics.rs), with a whole-tree fallback. On flush the owner translates into AccessKit: roles come from a single-valued cascade over the flags, and IDs are stable AccessibilityNodeIds. It diffs against a mirror of the last published accesskit::Node per node and publishes incrementally through a Send + Sync callback. flui-platform takes the translated accesskit::TreeUpdate through the PlatformAccessibility trait. That trait has UIA, NSAccessibility and AT-SPI adapters behind the `a11y` feature, which is off by default, plus a FakeAccessibility on headless. flui-app's per-presentation SemanticsHost turns semantics on only when the platform reports an active assistive technology. Inbound AccessKit actions are mapped to SemanticsAction and committed at the pipeline's Idle point.

The agent protocol (ADR-0080) is implemented today only in tools/desktop-mcp, over OS UI Automation. That tool keeps its own hand-copied subset of AccessKit roles, action names shaped after UIA patterns, and an outline format. flui-testing::a11y queries accesskit::TreeUpdate directly. flui-semantics' own SemanticsSnapshot has no user outside the crate.

In short, the protocol mechanics are solid: incremental publishing, stable IDs, per-realm ownership, live device checks (windows-a11y, windows-input). What is thin is what the catalog actually publishes, and how actions get back into state that lives on the owner thread. In flui-widgets only Focus and GestureDetector publish semantics. Only two render-object files describe semantics (the semantics proxy and the paragraph). EditableText publishes no text-field node. Action handlers have to be Arc<dyn Fn + Send + Sync>, so every widget that wants assistive-technology activation builds an AtomicBool-mailbox → rebuild → post-frame bridge. For the same reason the onFocus action is still not wired (ADR-0079). The pieces needed for "AccessKit by default" and "semantics is the agent vocabulary" are present. Three things are missing: a default-on build, a way to turn semantics on without an assistive technology, and one shared vocabulary. The catalog also does not yet fill the tree.

## Responsibilities and boundaries

OWNS today. flui-interaction: pointer and gesture pipeline (arena, recognizers, GestureBinding with hit-route retention and coalescing), velocity and resampling math, focus tree plus key bubbling (FocusManager, FocusNode, FocusScopeNode, traversal policy, claim_unfocused_keys), mouse enter/exit tracking, HitTestResult/HitTestEntry (re-exported by flui-rendering/src/hit_testing/{entry,result}.rs), the IME connection owner (text_input.rs), and InteractionLane (a TLS registry of owner-local closures). flui-semantics: the semantics data model, the tree arena, SemanticsOwner (publish, diff, focus derivation, action resolution), and AccessKit translation. flui-rendering: semantics assembly (walk, merge, geometry) and ownership of SemanticsOwner. flui-widgets: Semantics, MergeSemantics and ExcludeSemantics widgets, Focus/FocusScope widgets, Shortcuts, Actions, Intent, and GestureDetector's semantics bridge. flui-app: SemanticsHost (enablement), the platform bridge wiring (presentation.rs:372-420), and construction of FocusManager, GestureBinding and TextInputOwner per presentation (presentation.rs:446, 498-499). tools/desktop-mcp: the only implementation of the agent wire vocabulary.

WHERE THE BOUNDARIES LEAK. (1) InteractionLane is a general closure registry for paint concerns as well (clip paths, shader masks: interaction_lane.rs:158-190, 1757-1771). It exists because of the Send + Sync pin on render objects. It is not really an interaction concept, and it is reached through ambient TLS. (2) Semantics action handlers do NOT use that lane. They are Arc Send + Sync closures stored in SemanticsConfiguration (action.rs:217), so the owner-thread model is broken in two different ways. (3) flui-interaction depends on the whole flui-platform crate for a single trait (text_input.rs:27). (4) The command vocabulary (Intent/Action) sits in widgets and uses TypeId, so no lower layer, platform menu or agent can name a command. (5) The agent vocabulary sits in a tool (tools/desktop-mcp/src/a11y/role.rs) and not in a crate the in-process backend, testing or devtools could share.

WHAT BELONGS ELSEWHERE. The owner-lane callback registry should move to the view or rendering runtime, or into foundation, as an explicit handle passed in paint/hit-test contexts. The PlatformTextInput trait should move to a small capability-trait layer, which also sets the PlatformCapability pattern for H1. The agent/protocol vocabulary (serde roles, actions, states, outline) should move to one low-layer module that desktop-mcp, flui-testing, devtools and a2ui all import. Shortcuts and Actions can stay in widgets, but intents need stable names.

## Key types and contracts

- flui_interaction::GestureBinding (binding.rs, 4093 lines): owner-local, !Send; resolves and retains the Down hit route, coalesces/resamples Moves, orders route → arena lifecycle
- flui_interaction::GestureArena / GestureArenaMember (sealed) + CustomGestureRecognizer blanket impl: the only sanctioned gesture extension point; internally Arc + parking_lot::Mutex + DashMap under a crate-wide #![expect(clippy::arc_with_non_send_sync)] (lib.rs:143)
- flui_interaction::FocusManager (routing/focus.rs:63): one per presentation (flui-app presentation.rs:498); synchronous request_focus with FIFO reentrant queue bounded at REENTRANT_FOCUS_DRAIN_BUDGET=32; dispatch_key_event bubbles leaf→root (ADR-0023); claim_unfocused_keys (ADR-0079)
- flui_interaction::InteractionLane + InteractionDispatchHandle (#[doc(hidden)] pub, interaction_lane.rs:738-790): thread_local LOCAL_LANES/ACTIVE_LANES; opaque ScrollTarget/PathClipTarget/ShaderMaskTarget resolved via active_dispatch_handle() (interaction_lane.rs:1745-1771)
- flui_interaction::TextInputOwner / TextInputHandle / ClientToken: presentation-owned IME connection holding Arc<dyn flui_platform::traits::PlatformTextInput>; token-guarded detach; typed TextInputError::Unsupported
- flui_view::RenderView::RenderObject: RenderObject<P> + Send + Sync + 'static (flui-view/src/view/render.rs:451) — the root constraint behind the lane and the Send+Sync semantics handlers
- flui_semantics::SemanticsConfiguration / SemanticsFlag bitset / SemanticsAction (u64 bitmask, 24 variants, action.rs:23) / SemanticsRole (33 variants): Flutter-shaped internal model
- flui_semantics::SemanticsActionHandler = Arc<dyn Fn(SemanticsAction, Option<ActionArgs>) + Send + Sync> (action.rs:217); SemanticsUpdateCallback = Arc<dyn Fn(&TreeUpdate) + Send + Sync> (owner.rs:41)
- flui_semantics::SemanticsOwner (owner.rs:185): tree + PublishedState mirror (FxHashMap<accesskit::NodeId, accesskit::Node>) + focus_claimants; flush() diffs O(dirty); lives as Option on flui-rendering PipelineOwner (accessors.rs:1561)
- flui_semantics::{tree_to_update, semantics_action_for, semantics_action_args_for} (accesskit_translation.rs): resolve_role single-valued cascade (:65), explicit_role (:141), inbound action map (:299-318)
- flui_platform::traits::PlatformAccessibility (traits/accessibility.rs:57): publish(TreeUpdate), is_active, set_activation_listener, set_action_listener — the seam where AccessKit crosses; FakeAccessibility on headless
- flui-app SemanticsHost (semantics_host.rs): per-presentation enablement; SemanticsHandle is dead in production (#[expect(dead_code)] :30-38); announce() has no production caller
- flui_widgets Intent: Any (actions.rs:59), Action<T: Intent>, Actions map keyed by TypeId; Shortcuts resolves at the primary focus via FocusNode NodeContext (ADR-0079)
- flui_testing::a11y::A11yTree: AccessKit-TreeUpdate-based query API (find by role/label, supports_action, describe)
- tools/desktop-mcp AccessibilityBackend trait (a11y/mod.rs:498) + hand-written Role enum (a11y/role.rs:17) + ADR-0080 wire contract (handles, snake_case AccessKit roles, UIA-shaped action names, error codes)

## Dependencies

flui-interaction (layer 2). Normal dependencies: flui-types, flui-foundation, flui-platform (only for PlatformTextInput, text_input.rs:27), ui-events, cursor-icon, parking_lot, dashmap, smallvec, tracing, thiserror, web-time, and dpi (pinned directly, not through the workspace). `cargo tree -p flui-interaction -e normal` gives 76 unique lines, most of them pulled in through flui-platform. Dev-only cycle with flui-testing. Dependents: flui-rendering (re-exports HitTestResult/HitTestEntry/HitTestBehavior/PointerEvent in hit_testing/mod.rs:82-99), flui-objects, flui-view (FocusManager in contexts, RenderObjectContext carries InteractionDispatchHandle), flui-widgets, flui-material, flui-app, flui-testing, and the facade.

flui-semantics (layer 3). Dependencies: flui-foundation, flui-tree, flui-types, accesskit 0.25 (workspace), plus slab, parking_lot, tracing, smol_str, smallvec and rustc-hash pinned directly and not inherited from the workspace (flui-semantics/Cargo.toml). `cargo tree` gives 54 unique lines. Dependents: flui-rendering (which re-exports the crate as `flui_rendering::semantics`, lib.rs:80, and is how widgets reach it), flui-app, flui-testing, and the facade.

flui-platform (layer 2) does not depend on flui-semantics. It depends on accesskit directly and receives an already-translated TreeUpdate. The adapters are accesskit_windows (0.35), accesskit_macos and accesskit_unix (0.23), all optional behind `a11y`. There are no Android, iOS, web or winit adapters. tools/desktop-mcp depends on neither crate, nor on accesskit.

## Fit with the plan

H0 (beta; B1 exit is "Windows + Narrator + Japanese IME"; G1-G3 agent protocol). The route from the semantics pipeline to the OS works and is proven live: `cargo xtask device windows-a11y` and `windows-input`, per the journal and ADR-0079. The B1 form exit and the G2/G3 agent scenarios are blocked by gaps in this area, not by the pipeline. EditableText publishes no text-field semantics, so a screen reader and a semantics-driven agent cannot find, read or set a field. Scrollables publish no scroll actions, routes and dialogs do not scope or block, and onFocus is not wired. AccessKit is off by default. Semantics is not built unless an assistive technology activates it, so an in-process agent backend, devtools, or a semantic golden in a real app has no production way to switch it on. The "one protocol for devtools, agent and tests" idea (plan principle 4, roadmap G1) currently has four representations: the Flutter-shaped internal model, AccessKit, desktop-mcp's hand-copied vocabulary, and the unused SemanticsSnapshot.

H1 (mobile, PlatformCapability, A2UI). There is no AccessKit adapter for Android, iOS or web, and plan table stakes call for an iOS accessibility tree. TextInputOwner is the one worked example of a platform capability owned by a presentation (typed Unsupported, no string channels), which is a good template for PlatformCapability. It is placed wrongly, though: a layer-2 gesture crate depends on the whole platform crate. A2UI with "G6 catalog = A2UI catalog" needs semantics as a property of each catalog widget. Most catalog widgets have none today.

H2 (performance and scale). The semantics mirror keeps a second full copy of every accesskit::Node. The whole-tree fallback and the lock-heavy arena are unmeasured at 100k rows. The publish_cost bench exists and is the right place to measure. Keeping paint-time closure resolution tied to owner-thread TLS limits any later move of paint off the owner thread (hypothesis).

H3 (freeze). The public surface is large, with a lot that nothing production calls (EventRouter, HitTestable/CustomHitTestable, InputPredictor, RawInputHandler, OneEuroFilter, SemanticsSnapshot, SemanticsTreeUpdateBuilder, among others). Freezing it would lock in shapes that were never exercised.

H4 and extension points. CustomGestureRecognizer is a real, sealed extension point. CustomHitTestable is sealed but has no external implementors. There is no extension point for custom semantics roles or actions beyond CustomAction. There is no named command registry that menus (F8), agents or plugins could target.

Principles. Principle 3 (no global state) is mostly met: focus, gestures, IME and semantics are per presentation. It is dented by the ACTIVE_LANES and LOCAL_LANES TLS and the process-wide ID atomics. Principle 4 (everything machine-readable) is not met for intents, which are keyed by TypeId, or for the vocabulary, which is split across four representations.

## Strengths

- Ownership per presentation is real: FocusManager, GestureBinding and TextInputOwner are built per presentation (flui-app presentation.rs:446, 498-499, 644-645), and SemanticsHost replaced the retired process-wide SemanticsBinding (semantics_host.rs:1-14).
- The AccessKit boundary is placed well. flui-platform (layer 2) never names flui-semantics and receives a translated accesskit::TreeUpdate through PlatformAccessibility (traits/accessibility.rs:57). Headless has a FakeAccessibility. That seam is exactly where an in-process agent backend could plug in.
- Incremental publishing is mature: IDs stable across frames, an O(dirty) diff against a published mirror, focus derived incrementally, and a forced full republish when an assistive technology reattaches (owner.rs:185-260; presentation.rs:376-390). A publish_cost bench exists.
- Mapping decisions are recorded carefully and tested against the reference: the role-precedence cascade, static text value for UIA/AT-SPI, and mark-scoped assembly (flui-semantics/ARCHITECTURE.md decisions 1-4), with live device checks behind them (`cargo xtask device windows-a11y`, `windows-input`).
- Key dispatch and focus semantics are close to Flutter's contract and the gaps are documented: bubbling with KeyEventResult::combine (ADR-0023), intents resolved at the primary focus plus a key target while nothing is focused (ADR-0079), and a bounded reentrant-focus FIFO backed by a survey of six frameworks (interaction docs/ARCHITECTURE.md).
- Input vocabulary is shared: pointer and keyboard events are W3C ui-events types on both the platform side and the interaction side (events.rs header; flui-platform Cargo.toml:40), with no hand-rolled duplicate.
- TextInputOwner is a clean capability pattern: owner-local Weak handles, token-guarded detach, and typed Unsupported/Closed/OwnerGone errors (text_input.rs:1-80). It is a good template for H1 PlatformCapability.
- The executable gesture graph is intentionally !Send, and compile-time assertions pin that for the data-plane types only (lib.rs static_assertions; interaction_lane.rs tests assert_not_impl_any!(HandlerCell: Send, Sync)).

## Problems

### The catalog barely publishes semantics: no text fields, no scroll actions, no route or dialog scoping

- **Kind:** missing_capability · **Severity:** critical
- **Evidence:** In flui-widgets only interaction/focus.rs and interaction/gesture_detector.rs construct Semantics outside the semantics/ module (grep over crates/flui-widgets/src). Only two render-object files implement describe_semantics_configuration: flui-objects proxy/semantics.rs and text/paragraph.rs. crates/flui-widgets/src/text/editable_text.rs (4725 lines) and flui-objects text/editable.rs set no IsTextField flag, no value, and no SetText/SetSelection handler: nothing outside flui-semantics and tests matches SemanticsAction::SetText or SetSelection. Semantics::on_scroll_up/down and scopes_route/names_route exist (widgets/src/semantics/mod.rs:311-390) but no scrollable, route or overlay calls them; overlay/ and image/ never mention semantics. ARCHITECTURE decision 3 lists BlockSemantics (blocking previously painted siblings) as not implemented. Only 8 test files in the catalogs inspect an A11yTree.
- **Impact:** This blocks the B1 exit (a form operated with Narrator plus IME), the G2/G3 agent scenarios (set_value, type, scroll_into_view and scroll resolve against semantics), the table-stakes target of an a11y snapshot per widget in CI, and H1 A2UI, where the catalog is the contract. A modal dialog does not hide the background from assistive technology.
- **Direction:** Treat semantics as part of the definition of done for a widget: every catalog widget either publishes a SemanticsConfiguration or declares that it has none, and a generated test lists all catalog widgets and fails on any undeclared one. Wire EditableText (TextInput or MultilineTextInput role, value, text selection, SetText/SetSelection, and AccessKit text runs), Scrollable (scroll actions and SetScrollOffset), ModalRoute/Overlay (scopes_route, names_route, BlockSemantics) and Image (label) first.

### Semantics action handlers must be Send + Sync while the realm is owner-local, so each widget builds its own mailbox and onFocus stays unwired

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** flui-view/src/view/render.rs:451 pins RenderView::RenderObject to Send + Sync. SemanticsActionHandler is Arc<dyn Fn + Send + Sync> (flui-semantics/src/action.rs:217). flui-widgets/src/semantics/mod.rs:7-35 records that the ordinary Rc<RefCell> toggle closure does not compile and says to use Arc<Mutex>. GestureDetector builds an AtomicBool mailbox, then a rebuild, then drains in build(), then runs the callback post-frame (gesture_detector.rs:446-570). ADR-0079 'Not implemented' says the onFocus action is blocked because the handler must be Send + Sync while the node is owner-local; journal.md:17 confirms UIA reports buttons as not focusable.
- **Impact:** An action from assistive technology or an agent takes at least one extra frame. Every widget that wants semantic actions (slider increase/decrease, text set, scroll, expand) has to repeat the mailbox pattern. Side effects are scheduled from build(). This undercuts the agent protocol, whose act path is semantic actions, and makes widget authoring for third parties (H4) awkward.
- **Direction:** Route semantic actions like the other owner-local callbacks: a SemanticsActionTarget ID stored in SemanticsConfiguration and resolved on the owner lane at the Idle commit point, where SemanticsOwner::resolve_action already runs. Then handlers become Rc<dyn Fn> with direct state access, and the onFocus action can call FocusNode::request_focus. Record the change in an ADR that updates the Send + Sync render-object contract for callbacks.

### AccessKit is off by default, one feature covers all OSes, and mobile and web have no adapters

- **Kind:** plan_misfit · **Severity:** high
- **Evidence:** Root Cargo.toml:632-635 `a11y = ["flui-app/a11y"]` is off by default. flui-app/Cargo.toml:74 and flui-platform/Cargo.toml:310 `a11y = ["dep:accesskit_unix", "dep:accesskit_windows", "dep:accesskit_macos"]`, justified by the D-Bus stack of about 66 crates for Linux (flui-platform/Cargo.toml:216-221). AccessKit is used only in platforms/{windows,macos,linux,headless}; there is no accesskit_android, no iOS bridge, no web, and nothing on the winit fallback.
- **Impact:** This contradicts the plan's 'AccessKit by default by beta' (EAA-driven) and the goal of a screen reader passing a typical app on three desktops. A default `cargo add flui` app is invisible to Narrator and VoiceOver. H1 mobile has no accessibility path at all.
- **Direction:** Make accesskit_windows and accesskit_macos unconditional target-specific dependencies, since their cost is small and not the reason for the feature. Keep only AT-SPI behind a default-on feature that can be switched off (e.g. `a11y-linux`), or default it on with an opt-out. Put accesskit_android on the H1 plan, and a custom UIAccessibility bridge for iOS, since AccessKit has no iOS adapter (hypothesis: check upstream status). Add a CI check that the default feature set builds the Windows and macOS adapters.

### The semantics tree exists only while an OS assistive technology is active, so in-process agents, devtools and apps cannot turn it on

- **Kind:** missing_capability · **Severity:** high
- **Evidence:** flui-app semantics_host.rs:30-50: SemanticsHandle and ensure_semantics are #[expect(dead_code)] outside tests, with the reason 'only reached from tests until a production caller wires accessibility-handle acquisition'. The only production enablement is the platform activation listener (presentation.rs:376-395). announce() has no production caller (semantics_host.rs:110-114). SemanticsOwner is created only when enabled (flui-rendering pipeline/owner/accessors.rs:1561; ARCHITECTURE decision 3).
- **Impact:** ADR-0080's second backend (the in-process realm) and G1 devtools need a tree whether or not a screen reader runs. So do G3 semantic goldens in a real app, and record/replay (G7) keyed by semantic identity. Live-region announcements (snackbars, validation errors) never reach the OS.
- **Direction:** Add a realm-level enablement capability: a SemanticsHandle acquired through LifecycleContext and by the agent/devtools session, reference-counted with the platform flag. Deliver announcements as AccessKit live-region nodes through the existing publish path instead of a separate callback. Measure the cost of always-on with the publish_cost bench to decide the default for debug builds.

### There is no single machine vocabulary: four representations of the same tree

- **Kind:** extension_point · **Severity:** high
- **Evidence:** (1) Internal model shaped like Flutter: SemanticsFlag bitset, SemanticsAction bitmask with 24 variants, SemanticsRole with 33 (flui-semantics action.rs:23, role.rs:32). (2) AccessKit output through a precedence cascade (accesskit_translation.rs:65-141). (3) tools/desktop-mcp/src/a11y/role.rs:17 hand-copies about 40 AccessKit role names, with action names shaped after UIA patterns (invoke/toggle/select/expand/collapse, ADR-0080 Vocabulary). desktop-mcp does not depend on accesskit or flui-semantics. (4) flui-testing A11yTree::describe prints accesskit Role with Debug (CamelCase, a11y.rs:326), while flui-semantics SemanticsSnapshot (snapshot.rs, 665 lines) has no user outside the crate. ADR-0080 promises that 'the in-process backend speaks the same one', but the vocabulary sits in a tool binary.
- **Impact:** G1, G2, G3, G6 and A2UI each have to invent or re-copy a mapping, so names drift between tests, devtools, the MCP server and A2UI. Plan principle 4 and 'AccessKit vocabulary = agent vocabulary' are not structural yet. After 1.0 each copy is a breaking surface.
- **Direction:** Create one protocol module in a low layer (e.g. a serde-enabled `flui_semantics::protocol`, or a small layer-3 crate): role and action names derived from accesskit enums with serde, a flat state set, and the outline serializer from ADR-0080. Make desktop-mcp, flui-testing::a11y, devtools and later flui-a2ui import it. Delete SemanticsSnapshot or make it that format. Longer term, decide explicitly (ADR) whether the internal model should store accesskit::Role, accesskit::Action and states natively instead of Flutter flags plus a cascade.

### The internal action set cannot express part of the agent and AccessKit action space

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** semantics_action_for (accesskit_translation.rs:299-318) drops Expand, Collapse, ShowTooltip, ReplaceSelectedText, ScrollToPoint, numeric SetValue and others (`_ => None`). SemanticsAction has no Expand or Collapse. accesskit Blur is mapped to DidLoseAccessibilityFocus, which is an accessibility-focus notification, not input blur. semantics_action_args_for drops NumericValue, ScrollUnit and ScrollHint (:320-359). ADR-0080 advertises expand, collapse, select and set_value as agent actions.
- **Impact:** Trees, menus, dropdowns and sliders (the C-track Dropdown and Slider are still to come) cannot be driven by assistive technology or agents through semantics, only by synthetic pointer input. Blur has no correct target.
- **Direction:** Extend SemanticsAction to cover the AccessKit action set 1:1, or adopt accesskit::Action directly per the vocabulary decision above. Give ActionArgs a numeric value, and add a test that each accesskit::Action either maps or is listed as deliberately unsupported.

### InteractionLane has become a general owner-local closure registry reached through ambient TLS, and it lives in the interaction crate

- **Kind:** layering · **Severity:** medium
- **Evidence:** interaction_lane.rs:738-742 thread_local LOCAL_LANES and ACTIVE_LANES. PathClipTarget and ShaderMaskTarget ('Render objects store this value instead of storing a Fn(Size) -> Path', :158-190) are resolved via active_dispatch_handle(), which reads ACTIVE_LANES (:1745-1771). Consumers: flui-objects proxy/clip.rs, physical_model.rs, shader_mask.rs, and flui-rendering pipeline/owner/paint.rs. InteractionLane is #[doc(hidden)] pub 'only for sibling FLUI crates' (:744-750). Static NEXT_LANE_ID and NEXT_FOCUS_NODE_ID are process atomics (focus_scope.rs:24).
- **Impact:** Paint semantics depend on the gesture crate. An ambient lookup contradicts principle 3 and 'rules as types'. Paint and hit-test fail at run time (InactiveRealm) instead of at compile time when called outside enter(). Any future move of paint or recording off the owner thread (H2 raster/IO lanes) has to untangle this (hypothesis).
- **Direction:** Move the owner-lane callback registry into the runtime layer that owns the realm (flui-view, or rendering's PipelineOwner). Pass it explicitly in PaintContext and HitTestContext instead of TLS. Keep only the pointer-specific targets in flui-interaction. Fold semantic action targets into the same registry (see the Send + Sync problem).

### flui-interaction depends on the whole flui-platform crate for one trait

- **Kind:** workspace_topology · **Severity:** medium
- **Evidence:** flui-interaction/Cargo.toml: `flui-platform = { path = "../flui-platform" }` with the comment 'Presentation-owned IME state holds the exact platform capability directly'. The only use is text_input.rs:27 `use flui_platform::traits::PlatformTextInput`. flui-platform is about 50k lines with 340 unsafe sites and OS bindings. `cargo tree -p flui-interaction -e normal` gives 76 unique crates.
- **Impact:** Gesture, focus and velocity logic cannot be built or reused without the OS backends, and every change to flui-platform rebuilds and retests the interaction stack. H1 PlatformCapability will copy this pattern (a crate taking a dependency on the full platform crate to reach one capability trait) unless it is corrected now.
- **Direction:** Split the capability traits (PlatformTextInput, PlatformAccessibility, and later the PlatformCapability traits) into a small substrate module or crate below flui-platform, or move TextInputOwner up into flui-view/flui-app. Record the rule in ADR-0041 or in the PlatformCapability ADR.

### Commands are not machine-readable: intents are keyed by TypeId and text editing bypasses intents

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** flui-widgets/src/interaction/actions.rs:59 `pub trait Intent: Any {}`, with the Actions map keyed by TypeId (:22-31, 178). EditableText handles Backspace, Delete, arrows, Home and End in a raw key handler (editable_text.rs:906, 1625-1690) and only mentions Flutter's intents in comments (:1530, 1648-1653).
- **Impact:** Native menus (F8), agents, keymap customization and plugins cannot list or invoke commands by a stable name, which misses principle 4. Apps cannot override text-editing keys through Actions the way they can in Flutter. ADR-0023 already flagged this as a precondition for text-editing intents.
- **Direction:** Give intents a stable identity (a derive producing a const name, plus an optional serde payload). Add a per-realm registry of available actions at the primary focus that devtools and MCP can list, and move EditableText's key handling to DefaultTextEditingShortcuts plus intents.

### The owner-local gesture engine is built on locks and Arc

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** lib.rs:139-143 has a crate-wide `#![expect(clippy::arc_with_non_send_sync)]` with 'a future focused pass can migrate the owner-local handle graph to Rc'. Counts of DashMap/Mutex outside tests: arena/mod.rs 17, arena/team.rs 10, recognizers/tap.rs 8, long_press 6, tap_and_drag 6, and more. The docs table says the arena slots and recognizer state are guarded by parking_lot::Mutex on a lane that is owner-local by design.
- **Impact:** Every pointer event takes locks it does not need and pays for DashMap, which runs against AGENTS.md ('a lock on per-node state touched inside [the frame path] puts contention on every frame'). The types advertise a thread-safety story they do not have, and the documented 'Deref swallows poison' friction exists only because of the locks.
- **Direction:** Migrate the arena and recognizers to Rc plus RefCell/Cell (they are already !Send). Keep the Send + Sync data plane (events, hit paths, IDs) as it is. Make it one tracked refactor with the existing gesture_arena and tap benches as the oracle.

### Large unwired public surface, including extension points no one uses

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** No production reference outside flui-interaction (grep over crates/*/src and src, excluding tests) for EventRouter, RawInputHandler, InputPredictor or GestureEvent. OneEuroFilter/OneEuroFilter2D is pub but never used, even inside the crate. HitTestable/CustomHitTestable is a sealed extension point whose only implementor is a mock (event_router.rs:253). In flui-semantics, SemanticsSnapshot, SemanticsTreeUpdateBuilder, SemanticsEventType, SemanticsTag, SemanticsSortKey, CustomSemanticsAction and AccessibilityFocusBlockType have no users outside the crate. The ARCHITECTURE doc lists 72 doctests still marked rust,ignore.
- **Impact:** This is the repository's most common defect class (AGENTS.md review guidelines). At the H3 freeze it becomes Stable or Evolving surface that was never exercised, and it misleads contributors about which extension points are real.
- **Direction:** For each item, wire it (for example, SemanticsSortKey into traversal order, SemanticsSnapshot as the protocol format) or delete it before beta. Enforce with a public-surface reachability check in xtask, or with cargo-semver-checks plus an allowlist that states a reason for each entry.

### The semantics pipeline keeps three copies of every node and falls back to a whole-tree rebuild

- **Kind:** performance · **Severity:** medium
- **Evidence:** SemanticsOwner holds the SemanticsTree arena plus PublishedState { nodes: FxHashMap<NodeId, accesskit::Node> } (owner.rs:185-270), and the platform adapter keeps its own tree. Assembly falls back to a whole-tree rebuild whenever graft preconditions are not proven (ARCHITECTURE decision 3). A full publish translates every arena entry (owner.rs doc on examined_last_flush).
- **Impact:** Hypothesis: with AccessKit on by default, 100k-row lists (H2) and a 1 MB editor, publishing may cost noticeable memory and time. Virtualization limits node count, so this has to be measured, not assumed.
- **Direction:** Before making a11y default-on, add publish_cost bench scenarios for large virtualized lists and a text editor. Consider diffing against a hash per node instead of a clone, and track how often the whole-tree fallback fires with a counter visible in devtools.

### Gesture settings do not come from the OS

- **Kind:** flutter_divergence · **Severity:** low
- **Evidence:** GestureSettings is constructed only from defaults or touch_defaults(), and only in flui-app tests (runner/device_recovery.rs:927, frame_pacing.rs:1139). There are no platform queries for double-click time or drag threshold (no GetDoubleClickTime or equivalent in flui-platform/src).
- **Impact:** Double-click timing and drag slop ignore user and accessibility settings on Windows and macOS, a platform-fidelity gap and a small accessibility gap (users with motor impairments raise the double-click time). Hypothesis: impact is limited on desktop.
- **Direction:** Add a platform query (per window or per display) that feeds a per-realm GestureSettings, as Flutter does with MediaQuery.gestureSettings. Test it with the headless platform.

### Stale and inconsistent crate docs and manifests

- **Kind:** docs · **Severity:** low
- **Evidence:** interaction docs/ARCHITECTURE.md describes an 'owner-thread TLS FocusManager ... scheduled to move from ambient TLS into presentation ownership', but FocusManager is now plain per-presentation state (focus.rs:63; presentation.rs:498). The same doc names back-compat shims crate::team and crate::signal_resolver that lib.rs does not declare. The doc lives under docs/ rather than at the crate root, unlike flui-semantics. lib.rs:1 has the process marker 'Ship bar (wave 2)'. ADR-0026 says 'Absorbs: ADR-0026'. flui-semantics/Cargo.toml pins slab, parking_lot, tracing, smol_str, smallvec and rustc-hash directly instead of through [workspace.dependencies]. The interaction `serde` feature only forwards to flui-types, and `dpi` is pinned directly.
- **Impact:** Agents working from ARCHITECTURE.md (AGENTS.md: 'read the one for the crate you're changing') get a wrong ownership model, and manifest drift weakens the workspace contract.
- **Direction:** Refresh the interaction ARCHITECTURE.md (ownership table, subsystem list including InteractionLane, move to the crate root). Remove the marker. Move the direct dependencies to workspace inheritance, and add the check to `cargo xtask workspace` if it is not already enforced there.

### Oversized files hold the core interaction runtime

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** binding.rs 4093 lines, arena/mod.rs 2682, routing/focus.rs 2465, routing/interaction_lane.rs 2259, semantics configuration.rs 2159 (wc -l, inline tests included). In flui-widgets, editable_text.rs is 4725 and interaction/focus.rs 2383.
- **Impact:** Contributor ergonomics and review cost; a bus-factor-1 project depends on agents navigating these files. It also hides responsibilities that should be separate (the lane registry versus pointer dispatch).
- **Direction:** Split along real seams while doing the refactors above: GestureBinding into route retention, coalescing and resampling, and lifecycle ordering; the lane into a pointer part and a generic callback registry; focus.rs into manager, dispatch and notification queue.

## Unwired or dead surface

- flui_interaction::EventRouter (routing/event_router.rs, 313 lines): no production reference outside the crate
- flui_interaction::HitTestable / sealed::CustomHitTestable: a sealed extension point whose only implementor is a test mock (event_router.rs:253)
- flui_interaction::RawInputHandler / RawPointerEvent (processing/raw_input.rs, 700 lines): no external production user
- flui_interaction::InputPredictor / PredictionConfig (processing/prediction.rs, 539 lines): no external production user
- flui_interaction::processing::OneEuroFilter / OneEuroFilter2D (one_euro.rs, 256 lines): pub, used nowhere, even inside the crate
- flui_interaction::observability::GestureEvent: no external user; the ARCHITECTURE claim that flui-app surfaces it to devtools has no code behind it
- flui_semantics::SemanticsSnapshot / SemanticsNodeSnapshot (snapshot.rs, 665 lines): no user outside the crate
- flui_semantics::SemanticsTreeUpdateBuilder, SemanticsEventType, SemanticsTag, SemanticsSortKey, CustomSemanticsAction, AccessibilityFocusBlockType: no users outside the crate
- flui-app SemanticsHost::ensure_semantics / SemanticsHandle: #[expect(dead_code)] outside tests (semantics_host.rs:30-50)
- flui-app SemanticsHost::announce: no production caller (semantics_host.rs:110-114)
- flui_widgets Semantics::on_scroll_left/right/up/down, scopes_route, names_route: builders exist, but no catalog widget calls them
- flui-interaction `serde` feature: forwards only to flui-types/serde; no serde derives in the crate
- SemanticsAction::Focus handler path: advertised in the mapping (accesskit Focus→SemanticsAction::Focus) but no widget registers it (ADR-0079 'Not implemented')

## Open questions

- Should the internal semantics model stay Flutter-shaped (a flags bitset plus a precedence cascade to one AccessKit role), or become AccessKit-native (store accesskit::Role, Action and states directly)? The second makes 'AccessKit vocabulary = agent vocabulary' true by construction. The first keeps Flutter-test portability.
- Should the Send + Sync pin on render objects (flui-view render.rs:451) be kept at all, given that paint, hit-test and semantics all resolve owner-local closures anyway? What concrete off-thread consumer justifies it: the raster lane (ADR-0045)? Records only?
- Where should the agent protocol vocabulary crate or module sit (layer 3 next to flui-semantics, or in devtools), and does tools/desktop-mcp become a consumer of it or stay the source of truth?
- What does always-on semantics actually cost for an agent or devtools session (the publish_cost bench on a realistic Notes app and a 100k virtualized list)? This decides whether debug builds enable it by default.
- iOS: AccessKit has no iOS adapter (verify upstream). Is a FLUI-owned UIAccessibility bridge in H1 scope, and does it live in flui-platform under the same PlatformAccessibility trait?
- Is the D-Bus dependency the only real reason `a11y` is off, i.e. can Windows and macOS adapters become unconditional now without a measurable compile-time or binary cost? This has not been measured here.
- Should intents and commands (and a per-realm command registry) live in flui-interaction next to FocusManager, so that native menus (F8) and agents can target them without depending on flui-widgets?
- Does the CustomHitTestable extension point have any intended consumer (a third-party render catalog?), or should hit testing extension go only through RenderBox::hit_test in flui-rendering?

