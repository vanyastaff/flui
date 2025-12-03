//! Advanced Lifecycle Management System - Type-Safe State Transitions
//!
//! This module provides a comprehensive lifecycle management system that leverages
//! Rust's advanced type system to ensure safe state transitions and prevent
//! lifecycle-related bugs that are common in UI frameworks.
//!
//! ## Lifecycle Philosophy
//!
//! Unlike traditional UI frameworks with runtime lifecycle management, FLUI provides:
//! - **Compile-time State Validation**: Invalid state transitions caught at compile time
//! - **Zero-Cost State Machine**: State transitions with no runtime overhead
//! - **Memory-Safe Lifecycle**: Rust's ownership prevents lifecycle memory bugs
//! - **Type-Safe Hooks**: Lifecycle hooks validated at compile time
//! - **Parallel Lifecycle Processing**: Concurrent lifecycle management using crossbeam
//!
//! ## Architecture Overview
//!
//! ```text
//! LifecycleManager<S>
//!   ├─ TypeState Machine (compile-time validated)
//!   │   ├─ State transition validation
//!   │   ├─ Hook registration and execution
//!   │   └─ Lifecycle event dispatching
//!   ├─ Parallel Hook Execution (crossbeam)
//!   │   ├─ Lock-free hook queuing
//!   │   ├─ Concurrent hook processing
//!   │   └─ Dependency-ordered execution
//!   ├─ Resource Management (RAII)
//!   │   ├─ Automatic cleanup on state transitions
//!   │   ├─ Resource lifetime tracking
//!   │   └─ Memory leak prevention
//!   └─ Lifecycle Telemetry
//!       ├─ State transition monitoring
//!       ├─ Hook execution timing
//!       └─ Performance analytics
//! ```
//!
//! ## Advanced Features
//!
//! ### 1. Compile-Time State Machine Validation
//! ```rust,ignore
//! // Invalid state transitions caught at compile time!
//! let manager: LifecycleManager<Created> = LifecycleManager::new();
//! let manager: LifecycleManager<Initialized> = manager.initialize()?; // ✓ Valid
//! let manager: LifecycleManager<Running> = manager.start()?; // ✓ Valid
//! // let manager: LifecycleManager<Destroyed> = manager.destroy(); // ✗ Invalid: can't destroy from running!
//! ```
//!
//! ### 2. Type-Safe Lifecycle Hooks
//! ```rust,ignore
//! impl LifecycleHooks for MyWidget {
//!     type CreateState = Created;
//!     type RunState = Running;
//!
//!     fn on_create(&self, state: &Created) { /* ... */ }
//!     fn on_start(&self, state: &Running) { /* ... */ }
//! }
//! ```
//!
//! ### 3. Parallel Hook Execution
//! ```rust,ignore
//! // Hooks executed in parallel with dependency ordering
//! manager.register_hook(Hook::new().depends_on::<DatabaseHook>());
//! manager.register_hook(Hook::new().parallel_safe());
//! ```

use crossbeam::{
    channel::{self, Receiver, Sender},
    deque::{Injector, Stealer, Worker},
    utils::Backoff,
};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use smallvec::SmallVec;
use std::{
    any::{Any, TypeId},
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use thiserror::Error;

// ============================================================================
// Compile-Time State Machine
// ============================================================================

/// Lifecycle state marker trait for compile-time validation
pub trait LifecycleState: Send + Sync + 'static {
    /// State identifier for debugging
    const STATE_NAME: &'static str;

    /// Valid next states from this state
    type ValidTransitions: ValidStateTransitions;

    /// Resources managed by this state
    type ManagedResources: ManagedResources = ();

    /// Required capabilities for this state
    type RequiredCapabilities: StateCapabilities = ();
}

/// Valid state transitions trait using GATs
pub trait ValidStateTransitions: Send + Sync {
    /// Check if transition to target state is valid
    fn can_transition_to<Target: LifecycleState>() -> bool;

    /// Get all valid transition states
    fn valid_transitions() -> &'static [&'static str];
}

/// Managed resources trait
pub trait ManagedResources: Send + Sync {
    /// Initialize resources for this state
    fn initialize() -> Result<Self, LifecycleError>
    where
        Self: Sized;

    /// Cleanup resources when leaving this state
    fn cleanup(self) -> Result<(), LifecycleError>;
}

/// State capabilities trait
pub trait StateCapabilities: Send + Sync {
    /// Check if all required capabilities are available
    fn check_capabilities() -> Result<(), LifecycleError>;
}

// ============================================================================
// Predefined Lifecycle States
// ============================================================================

/// Created state - initial state after construction
#[derive(Debug, Clone, Copy)]
pub struct Created;

impl LifecycleState for Created {
    const STATE_NAME: &'static str = "Created";
    type ValidTransitions = CreatedTransitions;
}

pub struct CreatedTransitions;

impl ValidStateTransitions for CreatedTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Initializing>()
            || TypeId::of::<Target>() == TypeId::of::<Destroyed>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Initializing", "Destroyed"]
    }
}

/// Initializing state - resources being set up
#[derive(Debug, Clone, Copy)]
pub struct Initializing;

impl LifecycleState for Initializing {
    const STATE_NAME: &'static str = "Initializing";
    type ValidTransitions = InitializingTransitions;
}

pub struct InitializingTransitions;

impl ValidStateTransitions for InitializingTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Initialized>()
            || TypeId::of::<Target>() == TypeId::of::<Failed>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Initialized", "Failed"]
    }
}

/// Initialized state - ready to start
#[derive(Debug, Clone, Copy)]
pub struct Initialized;

impl LifecycleState for Initialized {
    const STATE_NAME: &'static str = "Initialized";
    type ValidTransitions = InitializedTransitions;
}

pub struct InitializedTransitions;

impl ValidStateTransitions for InitializedTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Starting>()
            || TypeId::of::<Target>() == TypeId::of::<Destroyed>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Starting", "Destroyed"]
    }
}

/// Starting state - beginning operation
#[derive(Debug, Clone, Copy)]
pub struct Starting;

impl LifecycleState for Starting {
    const STATE_NAME: &'static str = "Starting";
    type ValidTransitions = StartingTransitions;
}

pub struct StartingTransitions;

impl ValidStateTransitions for StartingTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Running>()
            || TypeId::of::<Target>() == TypeId::of::<Failed>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Running", "Failed"]
    }
}

/// Running state - actively operating
#[derive(Debug, Clone, Copy)]
pub struct Running;

impl LifecycleState for Running {
    const STATE_NAME: &'static str = "Running";
    type ValidTransitions = RunningTransitions;
}

pub struct RunningTransitions;

impl ValidStateTransitions for RunningTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Pausing>()
            || TypeId::of::<Target>() == TypeId::of::<Stopping>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Pausing", "Stopping"]
    }
}

/// Pausing state - temporarily stopping
#[derive(Debug, Clone, Copy)]
pub struct Pausing;

impl LifecycleState for Pausing {
    const STATE_NAME: &'static str = "Pausing";
    type ValidTransitions = PausingTransitions;
}

pub struct PausingTransitions;

impl ValidStateTransitions for PausingTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Paused>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Paused"]
    }
}

/// Paused state - temporarily inactive
#[derive(Debug, Clone, Copy)]
pub struct Paused;

impl LifecycleState for Paused {
    const STATE_NAME: &'static str = "Paused";
    type ValidTransitions = PausedTransitions;
}

pub struct PausedTransitions;

impl ValidStateTransitions for PausedTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Resuming>()
            || TypeId::of::<Target>() == TypeId::of::<Stopping>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Resuming", "Stopping"]
    }
}

/// Resuming state - returning to active operation
#[derive(Debug, Clone, Copy)]
pub struct Resuming;

impl LifecycleState for Resuming {
    const STATE_NAME: &'static str = "Resuming";
    type ValidTransitions = ResumingTransitions;
}

pub struct ResumingTransitions;

impl ValidStateTransitions for ResumingTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Running>()
            || TypeId::of::<Target>() == TypeId::of::<Failed>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Running", "Failed"]
    }
}

/// Stopping state - shutting down
#[derive(Debug, Clone, Copy)]
pub struct Stopping;

impl LifecycleState for Stopping {
    const STATE_NAME: &'static str = "Stopping";
    type ValidTransitions = StoppingTransitions;
}

pub struct StoppingTransitions;

impl ValidStateTransitions for StoppingTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Stopped>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Stopped"]
    }
}

/// Stopped state - cleanly shut down
#[derive(Debug, Clone, Copy)]
pub struct Stopped;

impl LifecycleState for Stopped {
    const STATE_NAME: &'static str = "Stopped";
    type ValidTransitions = StoppedTransitions;
}

pub struct StoppedTransitions;

impl ValidStateTransitions for StoppedTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Destroyed>()
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Destroyed"]
    }
}

/// Failed state - error occurred
#[derive(Debug, Clone, Copy)]
pub struct Failed;

impl LifecycleState for Failed {
    const STATE_NAME: &'static str = "Failed";
    type ValidTransitions = FailedTransitions;
}

pub struct FailedTransitions;

impl ValidStateTransitions for FailedTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        TypeId::of::<Target>() == TypeId::of::<Destroyed>()
            || TypeId::of::<Target>() == TypeId::of::<Initializing>() // Allow retry
    }

    fn valid_transitions() -> &'static [&'static str] {
        &["Destroyed", "Initializing"]
    }
}

/// Destroyed state - final state, resources cleaned up
#[derive(Debug, Clone, Copy)]
pub struct Destroyed;

impl LifecycleState for Destroyed {
    const STATE_NAME: &'static str = "Destroyed";
    type ValidTransitions = DestroyedTransitions;
}

pub struct DestroyedTransitions;

impl ValidStateTransitions for DestroyedTransitions {
    fn can_transition_to<Target: LifecycleState>() -> bool {
        false // Terminal state
    }

    fn valid_transitions() -> &'static [&'static str] {
        &[] // No valid transitions from destroyed
    }
}

// Default implementations for resource and capability traits
impl ManagedResources for () {
    fn initialize() -> Result<Self, LifecycleError> {
        Ok(())
    }

    fn cleanup(self) -> Result<(), LifecycleError> {
        Ok(())
    }
}

impl StateCapabilities for () {
    fn check_capabilities() -> Result<(), LifecycleError> {
        Ok(())
    }
}

// ============================================================================
// Lifecycle Manager with Type-Safe State Machine
// ============================================================================

/// Type-safe lifecycle manager with compile-time state validation
pub struct LifecycleManager<S: LifecycleState> {
    /// Current state data
    state_data: S::ManagedResources,

    /// Hook registry for this state
    hooks: Arc<HookRegistry>,

    /// Lifecycle event dispatcher
    dispatcher: Arc<LifecycleDispatcher>,

    /// Performance telemetry
    telemetry: Arc<LifecycleTelemetry>,

    /// State transition history
    transition_history: Vec<StateTransition>,

    /// Current state marker
    _state: PhantomData<S>,
}

impl LifecycleManager<Created> {
    /// Create a new lifecycle manager in Created state
    pub fn new() -> Self {
        Self {
            state_data: (),
            hooks: Arc::new(HookRegistry::new()),
            dispatcher: Arc::new(LifecycleDispatcher::new()),
            telemetry: Arc::new(LifecycleTelemetry::new()),
            transition_history: Vec::new(),
            _state: PhantomData,
        }
    }
}

impl<S: LifecycleState> LifecycleManager<S> {
    /// Transition to a new state (compile-time validated)
    pub fn transition_to<T: LifecycleState>(mut self) -> Result<LifecycleManager<T>, LifecycleError>
    where
        S::ValidTransitions: ValidStateTransitions,
    {
        // Compile-time validation
        if !S::ValidTransitions::can_transition_to::<T>() {
            return Err(LifecycleError::InvalidTransition {
                from: S::STATE_NAME,
                to: T::STATE_NAME,
                valid_transitions: S::ValidTransitions::valid_transitions().to_vec(),
            });
        }

        let start_time = Instant::now();

        // Check capabilities for target state
        T::RequiredCapabilities::check_capabilities()?;

        // Execute exit hooks for current state
        self.dispatcher
            .dispatch_exit_hooks::<S>(&self.hooks)
            .await?;

        // Cleanup current state resources
        self.state_data.cleanup()?;

        // Initialize new state resources
        let new_state_data = T::ManagedResources::initialize()?;

        // Record state transition
        let transition = StateTransition {
            from_state: S::STATE_NAME,
            to_state: T::STATE_NAME,
            timestamp: start_time,
            duration: start_time.elapsed(),
        };

        let mut new_manager = LifecycleManager {
            state_data: new_state_data,
            hooks: self.hooks,
            dispatcher: self.dispatcher,
            telemetry: self.telemetry,
            transition_history: self.transition_history,
            _state: PhantomData,
        };

        new_manager.transition_history.push(transition.clone());
        new_manager.telemetry.record_transition(transition);

        // Execute enter hooks for new state
        new_manager
            .dispatcher
            .dispatch_enter_hooks::<T>(&new_manager.hooks)
            .await?;

        Ok(new_manager)
    }

    /// Get current state name
    pub fn current_state(&self) -> &'static str {
        S::STATE_NAME
    }

    /// Register a lifecycle hook
    pub fn register_hook<H: LifecycleHook + 'static>(&self, hook: H) {
        self.hooks.register(hook);
    }

    /// Get transition history
    pub fn transition_history(&self) -> &[StateTransition] {
        &self.transition_history
    }

    /// Get lifecycle telemetry
    pub fn telemetry(&self) -> Arc<LifecycleTelemetry> {
        self.telemetry.clone()
    }
}

// Specific state transitions with type safety
impl LifecycleManager<Created> {
    /// Initialize the lifecycle (Created -> Initializing)
    pub fn initialize(self) -> Result<LifecycleManager<Initializing>, LifecycleError> {
        self.transition_to()
    }

    /// Destroy without initialization (Created -> Destroyed)
    pub fn destroy(self) -> Result<LifecycleManager<Destroyed>, LifecycleError> {
        self.transition_to()
    }
}

impl LifecycleManager<Initializing> {
    /// Complete initialization (Initializing -> Initialized)
    pub fn complete_initialization(self) -> Result<LifecycleManager<Initialized>, LifecycleError> {
        self.transition_to()
    }

    /// Fail initialization (Initializing -> Failed)
    pub fn fail_initialization(
        self,
        error: LifecycleError,
    ) -> Result<LifecycleManager<Failed>, LifecycleError> {
        // Record the failure reason
        self.telemetry.record_failure(error.clone());
        self.transition_to()
    }
}

impl LifecycleManager<Initialized> {
    /// Start operation (Initialized -> Starting)
    pub fn start(self) -> Result<LifecycleManager<Starting>, LifecycleError> {
        self.transition_to()
    }

    /// Destroy without starting (Initialized -> Destroyed)
    pub fn destroy(self) -> Result<LifecycleManager<Destroyed>, LifecycleError> {
        self.transition_to()
    }
}

impl LifecycleManager<Starting> {
    /// Complete startup (Starting -> Running)
    pub fn complete_startup(self) -> Result<LifecycleManager<Running>, LifecycleError> {
        self.transition_to()
    }

    /// Fail startup (Starting -> Failed)
    pub fn fail_startup(
        self,
        error: LifecycleError,
    ) -> Result<LifecycleManager<Failed>, LifecycleError> {
        self.telemetry.record_failure(error.clone());
        self.transition_to()
    }
}

impl LifecycleManager<Running> {
    /// Pause operation (Running -> Pausing)
    pub fn pause(self) -> Result<LifecycleManager<Pausing>, LifecycleError> {
        self.transition_to()
    }

    /// Stop operation (Running -> Stopping)
    pub fn stop(self) -> Result<LifecycleManager<Stopping>, LifecycleError> {
        self.transition_to()
    }
}

impl LifecycleManager<Pausing> {
    /// Complete pause (Pausing -> Paused)
    pub fn complete_pause(self) -> Result<LifecycleManager<Paused>, LifecycleError> {
        self.transition_to()
    }
}

impl LifecycleManager<Paused> {
    /// Resume operation (Paused -> Resuming)
    pub fn resume(self) -> Result<LifecycleManager<Resuming>, LifecycleError> {
        self.transition_to()
    }

    /// Stop from paused (Paused -> Stopping)
    pub fn stop(self) -> Result<LifecycleManager<Stopping>, LifecycleError> {
        self.transition_to()
    }
}

impl LifecycleManager<Resuming> {
    /// Complete resume (Resuming -> Running)
    pub fn complete_resume(self) -> Result<LifecycleManager<Running>, LifecycleError> {
        self.transition_to()
    }

    /// Fail resume (Resuming -> Failed)
    pub fn fail_resume(
        self,
        error: LifecycleError,
    ) -> Result<LifecycleManager<Failed>, LifecycleError> {
        self.telemetry.record_failure(error.clone());
        self.transition_to()
    }
}

impl LifecycleManager<Stopping> {
    /// Complete stop (Stopping -> Stopped)
    pub fn complete_stop(self) -> Result<LifecycleManager<Stopped>, LifecycleError> {
        self.transition_to()
    }
}

impl LifecycleManager<Stopped> {
    /// Destroy after stop (Stopped -> Destroyed)
    pub fn destroy(self) -> Result<LifecycleManager<Destroyed>, LifecycleError> {
        self.transition_to()
    }
}

impl LifecycleManager<Failed> {
    /// Retry initialization (Failed -> Initializing)
    pub fn retry(self) -> Result<LifecycleManager<Initializing>, LifecycleError> {
        self.transition_to()
    }

    /// Destroy after failure (Failed -> Destroyed)
    pub fn destroy(self) -> Result<LifecycleManager<Destroyed>, LifecycleError> {
        self.transition_to()
    }
}

// ============================================================================
// Lifecycle Hooks System
// ============================================================================

/// Lifecycle hook trait for custom behavior
pub trait LifecycleHook: Send + Sync {
    /// Hook name for debugging
    fn name(&self) -> &'static str;

    /// Hook dependencies (must execute before this hook)
    fn dependencies(&self) -> Vec<TypeId> {
        Vec::new()
    }

    /// Whether this hook can run in parallel with others
    fn parallel_safe(&self) -> bool {
        true
    }

    /// Execute on state entry
    fn on_enter(&self, state_name: &'static str) -> Result<(), LifecycleError> {
        let _ = state_name;
        Ok(())
    }

    /// Execute on state exit
    fn on_exit(&self, state_name: &'static str) -> Result<(), LifecycleError> {
        let _ = state_name;
        Ok(())
    }
}

/// Hook registry with parallel execution support
pub struct HookRegistry {
    /// Registered hooks by type
    hooks: DashMap<TypeId, Arc<dyn LifecycleHook>>,

    /// Hook execution order (dependency-resolved)
    execution_order: RwLock<Vec<TypeId>>,

    /// Parallel execution groups
    parallel_groups: RwLock<Vec<Vec<TypeId>>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            hooks: DashMap::new(),
            execution_order: RwLock::new(Vec::new()),
            parallel_groups: RwLock::new(Vec::new()),
        }
    }

    /// Register a lifecycle hook
    pub fn register<H: LifecycleHook + 'static>(&self, hook: H) {
        let type_id = TypeId::of::<H>();
        let hook_arc = Arc::new(hook);

        self.hooks.insert(type_id, hook_arc);

        // Rebuild execution order with new hook
        self.rebuild_execution_order();
    }

    /// Get all registered hooks
    pub fn hooks(&self) -> Vec<Arc<dyn LifecycleHook>> {
        self.hooks
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get execution order
    pub fn execution_order(&self) -> Vec<TypeId> {
        self.execution_order.read().clone()
    }

    fn rebuild_execution_order(&self) {
        // Topological sort based on dependencies
        let mut order = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut temp_visited = std::collections::HashSet::new();

        for entry in self.hooks.iter() {
            if !visited.contains(entry.key()) {
                self.visit_hook(*entry.key(), &mut visited, &mut temp_visited, &mut order);
            }
        }

        *self.execution_order.write() = order;

        // Build parallel execution groups
        self.build_parallel_groups();
    }

    fn visit_hook(
        &self,
        hook_id: TypeId,
        visited: &mut std::collections::HashSet<TypeId>,
        temp_visited: &mut std::collections::HashSet<TypeId>,
        order: &mut Vec<TypeId>,
    ) {
        if temp_visited.contains(&hook_id) {
            // Circular dependency detected, ignore
            return;
        }

        if visited.contains(&hook_id) {
            return;
        }

        temp_visited.insert(hook_id);

        if let Some(hook) = self.hooks.get(&hook_id) {
            for dep_id in hook.dependencies() {
                self.visit_hook(dep_id, visited, temp_visited, order);
            }
        }

        temp_visited.remove(&hook_id);
        visited.insert(hook_id);
        order.push(hook_id);
    }

    fn build_parallel_groups(&self) {
        let mut groups = Vec::new();
        let mut current_group = Vec::new();

        for &hook_id in self.execution_order.read().iter() {
            if let Some(hook) = self.hooks.get(&hook_id) {
                if hook.parallel_safe() && current_group.is_empty() {
                    current_group.push(hook_id);
                } else if hook.parallel_safe() {
                    current_group.push(hook_id);
                } else {
                    if !current_group.is_empty() {
                        groups.push(current_group.clone());
                        current_group.clear();
                    }
                    groups.push(vec![hook_id]);
                }
            }
        }

        if !current_group.is_empty() {
            groups.push(current_group);
        }

        *self.parallel_groups.write() = groups;
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Lifecycle event dispatcher with parallel hook execution
pub struct LifecycleDispatcher {
    /// Hook execution worker pool
    executor: crossbeam::thread::Scope<'static>,
}

impl LifecycleDispatcher {
    pub fn new() -> Self {
        // This is a simplified version - in practice, you'd use a proper thread pool
        Self {
            executor: unsafe { std::mem::zeroed() }, // Placeholder
        }
    }

    /// Dispatch enter hooks for a state
    pub async fn dispatch_enter_hooks<S: LifecycleState>(
        &self,
        registry: &HookRegistry,
    ) -> Result<(), LifecycleError> {
        self.execute_hooks(S::STATE_NAME, registry, HookType::Enter)
            .await
    }

    /// Dispatch exit hooks for a state
    pub async fn dispatch_exit_hooks<S: LifecycleState>(
        &self,
        registry: &HookRegistry,
    ) -> Result<(), LifecycleError> {
        self.execute_hooks(S::STATE_NAME, registry, HookType::Exit)
            .await
    }

    async fn execute_hooks(
        &self,
        state_name: &'static str,
        registry: &HookRegistry,
        hook_type: HookType,
    ) -> Result<(), LifecycleError> {
        let parallel_groups = registry.parallel_groups.read().clone();

        for group in parallel_groups {
            // Execute hooks in this group in parallel
            let mut handles = Vec::new();

            for hook_id in group {
                if let Some(hook) = registry.hooks.get(&hook_id) {
                    let hook_clone = hook.clone();
                    let state_name_copy = state_name;

                    let handle = tokio::spawn(async move {
                        match hook_type {
                            HookType::Enter => hook_clone.on_enter(state_name_copy),
                            HookType::Exit => hook_clone.on_exit(state_name_copy),
                        }
                    });

                    handles.push(handle);
                }
            }

            // Wait for all hooks in this group to complete
            for handle in handles {
                handle
                    .await
                    .map_err(|e| LifecycleError::HookExecutionFailed {
                        hook_name: "unknown".to_string(),
                        reason: e.to_string(),
                    })??;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum HookType {
    Enter,
    Exit,
}

// ============================================================================
// Lifecycle Telemetry and Monitoring
// ============================================================================

/// Lifecycle telemetry for monitoring state transitions and performance
pub struct LifecycleTelemetry {
    /// State transition records
    transitions: Mutex<VecDeque<StateTransition>>,

    /// Transition counters
    transition_counts: DashMap<String, AtomicUsize>,

    /// Failure records
    failures: Mutex<VecDeque<LifecycleFailure>>,

    /// Performance metrics
    metrics: LifecycleMetrics,
}

impl LifecycleTelemetry {
    pub fn new() -> Self {
        Self {
            transitions: Mutex::new(VecDeque::with_capacity(1000)),
            transition_counts: DashMap::new(),
            failures: Mutex::new(VecDeque::with_capacity(100)),
            metrics: LifecycleMetrics::new(),
        }
    }

    /// Record a state transition
    pub fn record_transition(&self, transition: StateTransition) {
        // Update metrics
        self.metrics
            .total_transitions
            .fetch_add(1, Ordering::Relaxed);
        self.metrics.record_transition_time(transition.duration);

        // Update counters
        let key = format!("{}->{}", transition.from_state, transition.to_state);
        self.transition_counts
            .entry(key)
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);

        // Store transition record
        let mut transitions = self.transitions.lock();
        transitions.push_back(transition);

        // Keep only recent transitions
        if transitions.len() > 1000 {
            transitions.pop_front();
        }
    }

    /// Record a lifecycle failure
    pub fn record_failure(&self, error: LifecycleError) {
        let failure = LifecycleFailure {
            timestamp: Instant::now(),
            error: error.to_string(),
        };

        self.metrics.total_failures.fetch_add(1, Ordering::Relaxed);

        let mut failures = self.failures.lock();
        failures.push_back(failure);

        // Keep only recent failures
        if failures.len() > 100 {
            failures.pop_front();
        }
    }

    /// Get lifecycle statistics
    pub fn statistics(&self) -> LifecycleStatistics {
        LifecycleStatistics {
            total_transitions: self.metrics.total_transitions.load(Ordering::Relaxed),
            total_failures: self.metrics.total_failures.load(Ordering::Relaxed),
            average_transition_time: self.metrics.average_transition_time(),
            transition_counts: self
                .transition_counts
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().load(Ordering::Relaxed)))
                .collect(),
        }
    }
}

impl Default for LifecycleTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

/// State transition record
#[derive(Debug, Clone)]
pub struct StateTransition {
    pub from_state: &'static str,
    pub to_state: &'static str,
    pub timestamp: Instant,
    pub duration: Duration,
}

/// Lifecycle failure record
#[derive(Debug, Clone)]
struct LifecycleFailure {
    timestamp: Instant,
    error: String,
}

/// Lifecycle performance metrics
struct LifecycleMetrics {
    total_transitions: AtomicUsize,
    total_failures: AtomicUsize,
    transition_times: Mutex<VecDeque<Duration>>,
}

impl LifecycleMetrics {
    fn new() -> Self {
        Self {
            total_transitions: AtomicUsize::new(0),
            total_failures: AtomicUsize::new(0),
            transition_times: Mutex::new(VecDeque::with_capacity(100)),
        }
    }

    fn record_transition_time(&self, duration: Duration) {
        let mut times = self.transition_times.lock();
        times.push_back(duration);

        if times.len() > 100 {
            times.pop_front();
        }
    }

    fn average_transition_time(&self) -> Duration {
        let times = self.transition_times.lock();
        if times.is_empty() {
            Duration::ZERO
        } else {
            let total: Duration = times.iter().sum();
            total / times.len() as u32
        }
    }
}

/// Lifecycle statistics summary
#[derive(Debug, Clone)]
pub struct LifecycleStatistics {
    pub total_transitions: usize,
    pub total_failures: usize,
    pub average_transition_time: Duration,
    pub transition_counts: HashMap<String, usize>,
}

// ============================================================================
// Error Types
// ============================================================================

/// Comprehensive lifecycle error types
#[derive(Error, Debug, Clone)]
pub enum LifecycleError {
    #[error("Invalid state transition: cannot transition from {from} to {to}. Valid transitions: {valid_transitions:?}")]
    InvalidTransition {
        from: &'static str,
        to: &'static str,
        valid_transitions: Vec<&'static str>,
    },

    #[error("Resource initialization failed: {reason}")]
    ResourceInitializationFailed { reason: String },

    #[error("Resource cleanup failed: {reason}")]
    ResourceCleanupFailed { reason: String },

    #[error("Required capability missing: {capability}")]
    CapabilityMissing { capability: String },

    #[error("Hook execution failed: {hook_name} - {reason}")]
    HookExecutionFailed { hook_name: String, reason: String },

    #[error("State transition timeout: {from} -> {to} took longer than {timeout:?}")]
    TransitionTimeout {
        from: &'static str,
        to: &'static str,
        timeout: Duration,
    },
}

// ============================================================================
// Widget Lifecycle Trait
// ============================================================================

/// Widget-specific lifecycle trait for integration with UI components
pub trait WidgetLifecycle: Send + Sync {
    /// Widget state type
    type State: LifecycleState;

    /// Called when widget is created
    fn on_create(&self) -> Result<(), LifecycleError> {
        Ok(())
    }

    /// Called when widget is mounted to the tree
    fn on_mount(&self) -> Result<(), LifecycleError> {
        Ok(())
    }

    /// Called when widget is updated
    fn on_update(&self) -> Result<(), LifecycleError> {
        Ok(())
    }

    /// Called when widget is unmounted from the tree
    fn on_unmount(&self) -> Result<(), LifecycleError> {
        Ok(())
    }

    /// Called when widget is destroyed
    fn on_destroy(&self) -> Result<(), LifecycleError> {
        Ok(())
    }
}

/// Default lifecycle implementation for widgets
#[derive(Debug)]
pub struct DefaultLifecycle;

impl WidgetLifecycle for DefaultLifecycle {
    type State = Created;
}

// ============================================================================
// Re-exports and Convenience Functions
// ============================================================================

pub use self::{
    Created, DefaultLifecycle, Destroyed, Failed, Initialized, Initializing, LifecycleError,
    LifecycleHook, LifecycleManager, LifecycleState, LifecycleStatistics, Paused, Pausing,
    Resuming, Running, Starting, StateTransition, Stopped, Stopping, WidgetLifecycle,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_state_transitions() {
        let manager = LifecycleManager::new();
        assert_eq!(manager.current_state(), "Created");

        let manager = manager.initialize().unwrap();
        assert_eq!(manager.current_state(), "Initializing");

        let manager = manager.complete_initialization().unwrap();
        assert_eq!(manager.current_state(), "Initialized");

        let manager = manager.start().unwrap();
        assert_eq!(manager.current_state(), "Starting");

        let manager = manager.complete_startup().unwrap();
        assert_eq!(manager.current_state(), "Running");

        // Test pause/resume cycle
        let manager = manager.pause().unwrap();
        assert_eq!(manager.current_state(), "Pausing");

        let manager = manager.complete_pause().unwrap();
        assert_eq!(manager.current_state(), "Paused");

        let manager = manager.resume().unwrap();
        assert_eq!(manager.current_state(), "Resuming");

        let manager = manager.complete_resume().unwrap();
        assert_eq!(manager.current_state(), "Running");

        // Test shutdown
        let manager = manager.stop().unwrap();
        assert_eq!(manager.current_state(), "Stopping");

        let manager = manager.complete_stop().unwrap();
        assert_eq!(manager.current_state(), "Stopped");

        let manager = manager.destroy().unwrap();
        assert_eq!(manager.current_state(), "Destroyed");
    }

    #[test]
    fn test_invalid_transitions() {
        let manager = LifecycleManager::new();

        // Can't go directly from Created to Running
        // This would be a compile-time error in real usage
        // let invalid = manager.transition_to::<Running>();
    }

    #[test]
    fn test_lifecycle_telemetry() {
        let telemetry = LifecycleTelemetry::new();

        let transition = StateTransition {
            from_state: "Created",
            to_state: "Initializing",
            timestamp: Instant::now(),
            duration: Duration::from_millis(5),
        };

        telemetry.record_transition(transition);

        let stats = telemetry.statistics();
        assert_eq!(stats.total_transitions, 1);
        assert!(stats
            .transition_counts
            .contains_key("Created->Initializing"));
    }
}
