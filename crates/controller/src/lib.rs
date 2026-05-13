use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use seriousum_core::controller::{
    ControllerConfig as SharedControllerConfig, ControllerStatus as SharedControllerStatus,
};

/// Convenience result type for the controller scaffold.
pub type Result<T> = anyhow::Result<T>;

/// Configuration for a single controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerConfig {
    /// Unique controller name.
    pub name: String,
    /// How often to run the control function. A zero duration runs once only.
    pub run_interval: Duration,
    /// Maximum number of retries on failure. `None` retries forever.
    pub max_retries: Option<u32>,
    /// Base retry interval used for exponential backoff after failures.
    pub error_retry_base: Duration,
    /// Maximum retry interval used for exponential backoff after failures.
    pub error_retry_max: Duration,
}

impl ControllerConfig {
    /// Create a controller configuration with Cilium-like defaults.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            run_interval: Duration::from_secs(60),
            max_retries: None,
            error_retry_base: Duration::from_secs(1),
            error_retry_max: Duration::from_secs(30),
        }
    }

    /// Override the steady-state run interval.
    #[must_use]
    pub fn with_interval(mut self, d: Duration) -> Self {
        self.run_interval = d;
        self
    }

    /// Cap the number of retries attempted after failures.
    #[must_use]
    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.max_retries = Some(n);
        self
    }
}

/// Compute exponential backoff as `base * 2^attempt`, capped at `max`.
#[must_use]
pub fn backoff_duration(base: Duration, max: Duration, attempt: u32) -> Duration {
    let multiplier = 1_u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX);
    base.saturating_mul(multiplier).min(max)
}

/// Current health and execution status for a controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControllerStatus {
    /// The controller has never executed.
    Idle,
    /// The controller is currently executing its reconcile loop.
    Running,
    /// The last controller run completed successfully.
    Success,
    /// The last controller run failed and will be retried.
    Failed {
        /// Number of consecutive failures since the last success.
        consecutive_errors: u32,
        /// Error string from the most recent failure.
        last_error: String,
    },
    /// The controller has been stopped.
    Stopped,
}

/// Runtime state tracked for a controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerState {
    /// Static controller configuration.
    pub config: ControllerConfig,
    /// Latest controller status.
    pub status: ControllerStatus,
    /// Timestamp of the last completed run.
    pub last_run: Option<SystemTime>,
    /// Number of successful runs.
    pub success_count: u64,
    /// Number of failed runs.
    pub failure_count: u64,
}

impl ControllerState {
    /// Create a new controller state in the idle state.
    #[must_use]
    pub fn new(config: ControllerConfig) -> Self {
        Self {
            config,
            status: ControllerStatus::Idle,
            last_run: None,
            success_count: 0,
            failure_count: 0,
        }
    }

    /// Record a successful controller execution.
    pub fn record_success(&mut self) {
        self.status = ControllerStatus::Success;
        self.last_run = Some(SystemTime::now());
        self.success_count += 1;
    }

    /// Record a failed controller execution.
    pub fn record_failure(&mut self, error: String) {
        let consecutive = match &self.status {
            ControllerStatus::Failed {
                consecutive_errors, ..
            } => consecutive_errors + 1,
            _ => 1,
        };
        self.status = ControllerStatus::Failed {
            consecutive_errors: consecutive,
            last_error: error,
        };
        self.last_run = Some(SystemTime::now());
        self.failure_count += 1;
    }

    /// Return whether the controller is currently healthy.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        matches!(
            self.status,
            ControllerStatus::Success | ControllerStatus::Idle
        )
    }

    /// Return the number of consecutive failures.
    #[must_use]
    pub fn consecutive_errors(&self) -> u32 {
        match &self.status {
            ControllerStatus::Failed {
                consecutive_errors, ..
            } => *consecutive_errors,
            _ => 0,
        }
    }
}

/// Errors returned by controller registry operations.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ControllerError {
    /// A controller with the given name already exists.
    #[error("controller already exists: {0}")]
    AlreadyExists(String),
    /// A controller with the given name could not be found.
    #[error("controller not found: {0}")]
    NotFound(String),
    /// A controller execution failed.
    #[error("controller execution error: {0}")]
    ExecutionError(String),
}

/// In-memory registry of named controller states.
#[derive(Debug, Default)]
pub struct ControllerManager {
    controllers: HashMap<String, ControllerState>,
}

impl ControllerManager {
    /// Create an empty controller registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new controller state.
    pub fn register(
        &mut self,
        config: ControllerConfig,
    ) -> std::result::Result<(), ControllerError> {
        if self.controllers.contains_key(&config.name) {
            return Err(ControllerError::AlreadyExists(config.name.clone()));
        }

        tracing::debug!(controller = %config.name, "registering controller");
        self.controllers
            .insert(config.name.clone(), ControllerState::new(config));
        Ok(())
    }

    /// Remove a controller from the registry.
    pub fn unregister(&mut self, name: &str) -> Option<ControllerState> {
        tracing::debug!(controller = %name, "unregistering controller");
        self.controllers.remove(name)
    }

    /// Fetch an immutable controller state reference.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ControllerState> {
        self.controllers.get(name)
    }

    /// Fetch a mutable controller state reference.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut ControllerState> {
        self.controllers.get_mut(name)
    }

    /// Return the number of registered controllers.
    #[must_use]
    pub fn count(&self) -> usize {
        self.controllers.len()
    }

    /// Return the names of all unhealthy controllers.
    #[must_use]
    pub fn unhealthy_controllers(&self) -> Vec<&str> {
        self.controllers
            .iter()
            .filter(|(_, state)| matches!(state.status, ControllerStatus::Failed { .. }))
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Return whether all registered controllers are healthy.
    #[must_use]
    pub fn all_healthy(&self) -> bool {
        self.controllers.values().all(ControllerState::is_healthy)
    }
}

// ---------------------------------------------------------------------------
// Error sentinels — match Go's errControllerNotFound / errControllerMapEmpty
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerNotFound;

impl fmt::Display for ControllerNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unable to find controller")
    }
}

impl std::error::Error for ControllerNotFound {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerMapEmpty;

impl fmt::Display for ControllerMapEmpty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "empty controller map")
    }
}

impl std::error::Error for ControllerMapEmpty {}

// ---------------------------------------------------------------------------
// ExitReason — returned from DoFunc to stop retrying (but keep goroutine alive)
// ---------------------------------------------------------------------------

/// When DoFunc returns this error the controller stops retrying and waits
/// for an explicit stop or update signal (mirrors Go's ExitReason).
#[derive(Debug)]
pub struct ExitReason(pub String);

impl fmt::Display for ExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exit reason: {}", self.0)
    }
}

impl std::error::Error for ExitReason {}

/// Construct a new ExitReason error (matches `NewExitReason` in Go).
pub fn new_exit_reason(reason: &str) -> anyhow::Error {
    anyhow::Error::new(ExitReason(reason.to_owned()))
}

// ---------------------------------------------------------------------------
// Cancellation token — a simple condvar-based cancel so DoFunc can observe
// stop signals (analogous to context.Context in Go).
// ---------------------------------------------------------------------------

/// A handle passed to DoFunc/StopFunc that signals cancellation.
/// Cheap to clone; `is_cancelled()` is non-blocking.
#[derive(Clone)]
pub struct CancelToken {
    inner: Arc<CancelTokenInner>,
}

struct CancelTokenInner {
    cancelled: Mutex<bool>,
    cv: Condvar,
}

impl CancelToken {
    fn new() -> Self {
        CancelToken {
            inner: Arc::new(CancelTokenInner {
                cancelled: Mutex::new(false),
                cv: Condvar::new(),
            }),
        }
    }

    /// Returns true if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        *self.inner.cancelled.lock().unwrap()
    }

    /// Block until cancelled.
    pub fn wait_for_cancel(&self) {
        let guard = self.inner.cancelled.lock().unwrap();
        let _guard = self.inner.cv.wait_while(guard, |c| !*c).unwrap();
    }

    fn cancel(&self) {
        let mut guard = self.inner.cancelled.lock().unwrap();
        *guard = true;
        self.inner.cv.notify_all();
    }
}

// ---------------------------------------------------------------------------
// ControllerFunc type alias
// ---------------------------------------------------------------------------

/// The signature that DoFunc / StopFunc must implement.
/// Uses Arc so it can be cheaply cloned out of the shared params without
/// holding the params lock during execution.
pub type ControllerFuncInner = dyn Fn(CancelToken) -> anyhow::Result<()> + Send + Sync + 'static;
pub type ControllerFunc = Arc<ControllerFuncInner>;

// ---------------------------------------------------------------------------
// NoopFunc
// ---------------------------------------------------------------------------

/// A no-op placeholder (matches `NoopFunc` in Go).
pub fn noop_func(_token: CancelToken) -> anyhow::Result<()> {
    Ok(())
}

// ---------------------------------------------------------------------------
// ControllerParams
// ---------------------------------------------------------------------------

/// Parameters for a controller.  DoFunc and StopFunc are `Arc<dyn Fn>` so they
/// can be cloned cheaply without holding the params lock during execution.
pub struct ControllerParams {
    /// The function run by the controller.  If None a no-op is used.
    pub do_func: Option<ControllerFunc>,
    /// Called once when the controller is stopped.
    pub stop_func: Option<ControllerFunc>,
    /// If non-zero, re-run DoFunc at this interval after each success.
    pub run_interval: Duration,
    /// Base wait duration between error retries.  Default → 1 s.
    pub error_retry_base_duration: Duration,
    /// When true, do not retry on error; wait for stop/update instead.
    pub no_error_retry: bool,
}

impl Default for ControllerParams {
    fn default() -> Self {
        ControllerParams {
            do_func: None,
            stop_func: None,
            run_interval: Duration::ZERO,
            error_retry_base_duration: Duration::ZERO,
            no_error_retry: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-controller stats (shared between the run-loop thread and Manager)
// ---------------------------------------------------------------------------

struct ControllerStats {
    success_count: usize,
    failure_count: usize,
    last_error: Option<String>,
}

// ---------------------------------------------------------------------------
// Shared params snapshot — written by Manager (under lock), cloned by run-loop
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ParamsSnapshot {
    do_func: Option<ControllerFunc>,
    stop_func: Option<ControllerFunc>,
    run_interval: Duration,
    error_retry_base_duration: Duration,
    no_error_retry: bool,
    generation: u64,
}

impl From<&ControllerParams> for ParamsSnapshot {
    fn from(p: &ControllerParams) -> Self {
        ParamsSnapshot {
            do_func: p.do_func.clone(),
            stop_func: p.stop_func.clone(),
            run_interval: p.run_interval,
            error_retry_base_duration: p.error_retry_base_duration,
            no_error_retry: p.no_error_retry,
            generation: 0,
        }
    }
}

struct SharedParams {
    snapshot: ParamsSnapshot,
}

// ---------------------------------------------------------------------------
// ManagedController — one running controller
// ---------------------------------------------------------------------------

struct ManagedController {
    /// Signals the run-loop to stop.
    stop_tx: std::sync::mpsc::SyncSender<()>,
    /// Non-blocking send to wake the run-loop for a params update.
    update_tx: std::sync::mpsc::SyncSender<()>,
    /// Closed (via condvar) when the run-loop terminates.
    terminated: Arc<(Mutex<bool>, Condvar)>,
    /// Current cancellation token — cancelled and replaced on each update.
    cancel_token: Arc<Mutex<CancelToken>>,
    /// Stats written by run-loop.
    stats: Arc<Mutex<ControllerStats>>,
    /// Params that the Manager may update; run-loop snaps and releases quickly.
    shared_params: Arc<Mutex<SharedParams>>,
}

// ---------------------------------------------------------------------------
// Run loop
// ---------------------------------------------------------------------------

/// "Infinity" wait: 24 h — safe to add to Instant::now().
const WAIT_FOREVER: Duration = Duration::from_secs(86_400);

fn run_controller(
    name: String,
    shared_params: Arc<Mutex<SharedParams>>,
    stats: Arc<Mutex<ControllerStats>>,
    stop_rx: std::sync::mpsc::Receiver<()>,
    update_rx: std::sync::mpsc::Receiver<()>,
    terminated: Arc<(Mutex<bool>, Condvar)>,
    cancel_token: Arc<Mutex<CancelToken>>,
) {
    let mut error_retries: u32 = 1;

    loop {
        // ── 1. Snapshot current params (release lock immediately). ──────────
        let snap = {
            let sp = shared_params.lock().unwrap();
            sp.snapshot.clone()
        };

        // ── 2. Get current cancel token. ────────────────────────────────────
        let token = cancel_token.lock().unwrap().clone();

        // ── 3. Call DoFunc (no lock held). ──────────────────────────────────
        let result = match &snap.do_func {
            Some(f) => f(token.clone()),
            None => Err(anyhow::anyhow!("controller {} DoFunc is nil", name)),
        };

        // ── 4. Determine next wait interval. ────────────────────────────────
        let interval;
        match result {
            Ok(()) => {
                {
                    let mut s = stats.lock().unwrap();
                    s.success_count += 1;
                    s.last_error = None;
                }
                error_retries = 1;

                if snap.run_interval == Duration::ZERO {
                    interval = WAIT_FOREVER;
                } else {
                    interval = snap.run_interval;
                }
            }
            Err(e) => {
                // An ExitReason or a cancelled context both mean "done for now".
                let is_exit = e.is::<ExitReason>() || token.is_cancelled();

                if is_exit {
                    {
                        let mut s = stats.lock().unwrap();
                        s.success_count += 1;
                        s.last_error = None;
                    }
                    interval = WAIT_FOREVER;
                } else {
                    {
                        let mut s = stats.lock().unwrap();
                        s.failure_count += 1;
                        s.last_error = Some(format!("{}", e));
                    }
                    if snap.no_error_retry {
                        interval = WAIT_FOREVER;
                    } else {
                        let base = if snap.error_retry_base_duration == Duration::ZERO {
                            Duration::from_secs(1)
                        } else {
                            snap.error_retry_base_duration
                        };
                        interval = base.saturating_mul(error_retries);
                        error_retries = error_retries.saturating_add(1);
                    }
                }
            }
        }

        // ── 5. Wait for stop / update / timer. ──────────────────────────────
        match wait_for_signal(&stop_rx, &update_rx, interval) {
            WaitResult::Stop => break,
            WaitResult::Update => {
                error_retries = 1;
                continue;
            }
            WaitResult::TimedOut => {
                // Race-check stop.
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                continue;
            }
        }
    }

    // ── Shutdown: run StopFunc. ──────────────────────────────────────────────
    let stop_token = CancelToken::new(); // fresh, never cancelled
    let stop_func = {
        let sp = shared_params.lock().unwrap();
        sp.snapshot.stop_func.clone()
    };
    if let Some(f) = stop_func {
        let _ = f(stop_token);
    }

    // ── Signal termination. ─────────────────────────────────────────────────
    let (lock, cv) = &*terminated;
    let mut done = lock.lock().unwrap();
    *done = true;
    cv.notify_all();
}

fn wait_for_signal(
    stop_rx: &std::sync::mpsc::Receiver<()>,
    update_rx: &std::sync::mpsc::Receiver<()>,
    interval: Duration,
) -> WaitResult {
    let deadline = std::time::Instant::now() + interval;
    loop {
        if stop_rx.try_recv().is_ok() {
            return WaitResult::Stop;
        }
        if update_rx.try_recv().is_ok() {
            return WaitResult::Update;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return WaitResult::TimedOut;
        }
        let remaining = deadline - now;
        std::thread::sleep(remaining.min(Duration::from_millis(1)));
    }
}

#[derive(Debug)]
enum WaitResult {
    Stop,
    Update,
    TimedOut,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// A Manager owns a set of named controllers and manages their lifecycle.
///
/// A zero-value `Manager::default()` has `controllers == None`; operations that
/// require an initialized map will return `ControllerMapEmpty`.
pub struct Manager {
    /// None means "zero-value / uninitialized" (matches Go's nil map).
    controllers: RwLock<Option<HashMap<String, ManagedController>>>,
}

impl Default for Manager {
    fn default() -> Self {
        Manager {
            controllers: RwLock::new(None),
        }
    }
}

impl Manager {
    /// Allocate a new, initialized Manager (matches `NewManager()` in Go).
    pub fn new() -> Self {
        Manager {
            controllers: RwLock::new(Some(HashMap::new())),
        }
    }

    /// Install or update a controller by name.  Returns a handle for inspecting
    /// stats (mirrors the internal `*managedController` return in Go).
    pub fn update_controller(&self, name: &str, params: ControllerParams) -> ControllerHandle {
        let mut controllers = self.controllers.write().unwrap();
        let map = controllers.get_or_insert_with(HashMap::new);

        if let Some(existing) = map.get(name) {
            // Update the shared snapshot.
            {
                let mut sp = existing.shared_params.lock().unwrap();
                let next_gen = sp.snapshot.generation + 1;
                sp.snapshot = ParamsSnapshot::from(&params);
                sp.snapshot.generation = next_gen;
                // Hand params ownership in (drop original).
                drop(params);
            }
            // Cancel the current token so an in-progress DoFunc observes it.
            let old_token = {
                let mut tok = existing.cancel_token.lock().unwrap();
                let old = tok.clone();
                *tok = CancelToken::new(); // fresh token for next run
                old
            };
            old_token.cancel();
            // Wake the run-loop (non-blocking; capacity-1 channel keeps latest).
            let _ = existing.update_tx.try_send(());

            ControllerHandle {
                stats: Arc::clone(&existing.stats),
            }
        } else {
            self.create_controller_locked(map, name, params)
        }
    }

    /// Install a controller only if one with that name does not yet exist.
    /// Returns `true` if created, `false` if already present.
    pub fn create_controller(&self, name: &str, params: ControllerParams) -> bool {
        let mut controllers = self.controllers.write().unwrap();
        let map = controllers.get_or_insert_with(HashMap::new);

        if map.contains_key(name) {
            return false;
        }

        self.create_controller_locked(map, name, params);
        true
    }

    fn create_controller_locked(
        &self,
        map: &mut HashMap<String, ManagedController>,
        name: &str,
        params: ControllerParams,
    ) -> ControllerHandle {
        let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(1);
        // Capacity 1: a second update before the first is consumed just replaces it.
        let (update_tx, update_rx) = std::sync::mpsc::sync_channel::<()>(1);

        let terminated = Arc::new((Mutex::new(false), Condvar::new()));
        let cancel_token = Arc::new(Mutex::new(CancelToken::new()));
        let stats = Arc::new(Mutex::new(ControllerStats {
            success_count: 0,
            failure_count: 0,
            last_error: None,
        }));
        let snap = ParamsSnapshot::from(&params);
        drop(params); // params consumed
        let shared_params = Arc::new(Mutex::new(SharedParams { snapshot: snap }));

        // Clone Arcs for the thread.
        let t_name = name.to_owned();
        let t_shared = Arc::clone(&shared_params);
        let t_stats = Arc::clone(&stats);
        let t_term = Arc::clone(&terminated);
        let t_token = Arc::clone(&cancel_token);

        std::thread::spawn(move || {
            run_controller(
                t_name, t_shared, t_stats, stop_rx, update_rx, t_term, t_token,
            );
        });

        let handle = ControllerHandle {
            stats: Arc::clone(&stats),
        };

        map.insert(
            name.to_owned(),
            ManagedController {
                stop_tx,
                update_tx,
                terminated,
                cancel_token,
                stats,
                shared_params,
            },
        );

        handle
    }

    /// Stop and remove a controller.  Returns `Err(ControllerNotFound)` if the
    /// name is not present, or `Err(ControllerMapEmpty)` if the map is nil.
    pub fn remove_controller(&self, name: &str) -> anyhow::Result<()> {
        let mut controllers = self.controllers.write().unwrap();
        match controllers.as_mut() {
            None => Err(anyhow::Error::new(ControllerMapEmpty)),
            Some(map) => {
                if let Some(ctrl) = map.remove(name) {
                    // Cancel in-progress DoFunc, then signal stop.
                    ctrl.cancel_token.lock().unwrap().cancel();
                    let _ = ctrl.stop_tx.try_send(());
                    Ok(())
                } else {
                    Err(anyhow::Error::new(ControllerNotFound))
                }
            }
        }
    }

    /// Like `remove_controller` but blocks until the controller thread exits.
    /// Returns `Ok(())` (no-op) if the controller is not found.
    pub fn remove_controller_and_wait(&self, name: &str) -> anyhow::Result<()> {
        let terminated_arc = {
            let mut controllers = self.controllers.write().unwrap();
            match controllers.as_mut() {
                None => return Err(anyhow::Error::new(ControllerMapEmpty)),
                Some(map) => {
                    if let Some(ctrl) = map.remove(name) {
                        ctrl.cancel_token.lock().unwrap().cancel();
                        let _ = ctrl.stop_tx.try_send(());
                        Arc::clone(&ctrl.terminated)
                    } else {
                        // Not found — no-op.
                        return Ok(());
                    }
                }
            }
        };

        // Wait for the thread to finish (outside the write lock).
        let (lock, cv) = &*terminated_arc;
        let guard = lock.lock().unwrap();
        let _guard = cv.wait_while(guard, |done| !*done).unwrap();
        Ok(())
    }

    /// Stop and remove all controllers.  Does not wait.
    pub fn remove_all(&self) {
        let mut controllers = self.controllers.write().unwrap();
        if let Some(map) = controllers.as_mut() {
            for (_, ctrl) in map.drain() {
                ctrl.cancel_token.lock().unwrap().cancel();
                let _ = ctrl.stop_tx.try_send(());
            }
        }
    }
}

/// Return a global status list (non-nil placeholder).
pub fn get_global_status() -> Vec<String> {
    vec![]
}

/// A handle returned by `update_controller` / `create_controller` so callers
/// can inspect stats without holding the Manager lock.
pub struct ControllerHandle {
    stats: Arc<Mutex<ControllerStats>>,
}

impl ControllerHandle {
    pub fn get_success_count(&self) -> usize {
        self.stats.lock().unwrap().success_count
    }

    pub fn get_failure_count(&self) -> usize {
        self.stats.lock().unwrap().failure_count
    }

    /// Returns `Ok(())` when there is no last error.
    pub fn get_last_error(&self) -> anyhow::Result<()> {
        match &self.stats.lock().unwrap().last_error {
            None => Ok(()),
            Some(e) => Err(anyhow::anyhow!("{}", e)),
        }
    }
}

// ---------------------------------------------------------------------------
// Original ControllerScaffold (unchanged)
// ---------------------------------------------------------------------------

/// Minimal controller scaffold built on the shared core controller.
#[derive(Clone)]
pub struct ControllerScaffold {
    controller: seriousum_core::Controller,
}

impl ControllerScaffold {
    /// Create a new scaffold from a controller name.
    pub fn new(name: impl Into<String>) -> Self {
        Self::with_config(SharedControllerConfig::new(name))
    }

    /// Create a new scaffold from an explicit config.
    pub fn with_config(config: SharedControllerConfig) -> Self {
        Self {
            controller: seriousum_core::Controller::new(config),
        }
    }

    /// Create the default scaffold.
    pub fn scaffold() -> Self {
        Self::new("seriousum-controller").with_group("controller")
    }

    /// Set the logical controller group.
    pub fn with_group(self, group: impl Into<String>) -> Self {
        let mut config = (*self.controller.config).clone();
        config.group = group.into();
        Self::with_config(config)
    }

    /// Set the controller rate limit.
    pub fn with_rate_limit(self, rate_limit: Duration) -> Self {
        let mut config = (*self.controller.config).clone();
        config.rate_limit = Some(rate_limit);
        Self::with_config(config)
    }

    /// Access the controller config.
    pub fn config(&self) -> SharedControllerConfig {
        (*self.controller.config).clone()
    }

    /// Access the controller status.
    pub fn status(&self) -> SharedControllerStatus {
        self.controller
            .status
            .try_read()
            .map_or(SharedControllerStatus::Stopped, |status| *status)
    }

    /// Delegate worker registration to the shared core controller.
    pub async fn set_worker<F, Fut>(&self, worker: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.controller.set_worker(worker).await;
    }

    /// Run the controller once.
    pub async fn run_once(&self) -> anyhow::Result<()> {
        self.controller.run_once().await
    }

    /// Stop the controller.
    pub async fn stop(&self) {
        self.controller.stop().await;
    }

    /// Build a concise report.
    pub fn report(&self) -> ControllerReport {
        let config = self.config();
        ControllerReport {
            name: config.name,
            group: config.group,
            rate_limit_ms: config
                .rate_limit
                .map(|duration| duration.as_millis() as u64),
            status: self.status(),
        }
    }

    /// Render the report as a string.
    pub fn summary(&self) -> String {
        self.report().to_string()
    }
}

/// Controller report rendered by the thin binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerReport {
    /// Controller name.
    pub name: String,
    /// Controller group.
    pub group: String,
    /// Optional rate limit in milliseconds.
    pub rate_limit_ms: Option<u64>,
    /// Current status.
    pub status: SharedControllerStatus,
}

impl fmt::Display for ControllerReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.rate_limit_ms {
            Some(rate_limit_ms) => write!(
                f,
                "controller scaffold ready | name={} | group={} | rate_limit_ms={} | status={}",
                self.name, self.group, rate_limit_ms, self.status,
            ),
            None => write!(
                f,
                "controller scaffold ready | name={} | group={} | status={}",
                self.name, self.group, self.status,
            ),
        }
    }
}

/// Run the controller scaffold.
pub fn run() -> Result<String> {
    Ok(ControllerScaffold::scaffold().summary())
}

// ---------------------------------------------------------------------------
// Controller model tests.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod controller_model_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_backoff_duration() {
        let base = Duration::from_millis(100);
        let max = Duration::from_secs(10);

        assert_eq!(backoff_duration(base, max, 0), Duration::from_millis(100));
        assert_eq!(backoff_duration(base, max, 1), Duration::from_millis(200));
        assert_eq!(backoff_duration(base, max, 3), Duration::from_millis(800));
        assert_eq!(backoff_duration(base, max, 20), max);
    }

    #[test]
    fn test_controller_state_success_failure() {
        let mut state = ControllerState::new(ControllerConfig::new("test"));

        assert!(state.is_healthy());
        state.record_failure("boom".into());
        assert!(!state.is_healthy());
        assert_eq!(state.consecutive_errors(), 1);

        state.record_failure("boom2".into());
        assert_eq!(state.consecutive_errors(), 2);

        state.record_success();
        assert!(state.is_healthy());
        assert_eq!(state.consecutive_errors(), 0);
        assert_eq!(state.success_count, 1);
        assert_eq!(state.failure_count, 2);
    }

    #[test]
    fn test_controller_manager_register() {
        let mut manager = ControllerManager::new();

        assert!(
            manager
                .register(ControllerConfig::new("sync-endpoints"))
                .is_ok()
        );
        assert!(
            manager
                .register(ControllerConfig::new("sync-policies"))
                .is_ok()
        );
        assert_eq!(manager.count(), 2);
        assert!(
            manager
                .register(ControllerConfig::new("sync-endpoints"))
                .is_err()
        );
    }

    #[test]
    fn test_unhealthy_controllers() {
        let mut manager = ControllerManager::new();
        manager.register(ControllerConfig::new("c1")).unwrap();
        manager.register(ControllerConfig::new("c2")).unwrap();

        manager
            .get_mut("c1")
            .unwrap()
            .record_failure("error".into());

        let unhealthy = manager.unhealthy_controllers();
        assert_eq!(unhealthy.len(), 1);
        assert_eq!(unhealthy[0], "c1");
        assert!(!manager.all_healthy());

        manager.get_mut("c1").unwrap().record_success();
        assert!(manager.all_healthy());
    }

    #[test]
    fn test_controller_config_builder() {
        let config = ControllerConfig::new("my-ctrl")
            .with_interval(Duration::from_secs(30))
            .with_max_retries(5);

        assert_eq!(config.name, "my-ctrl");
        assert_eq!(config.run_interval, Duration::from_secs(30));
        assert_eq!(config.max_retries, Some(5));
    }

    #[test]
    fn test_unregister() {
        let mut manager = ControllerManager::new();
        manager.register(ControllerConfig::new("c1")).unwrap();

        assert!(manager.unregister("c1").is_some());
        assert_eq!(manager.count(), 0);
        assert!(manager.unregister("c1").is_none());
    }
}

// ---------------------------------------------------------------------------
// Parity tests ported from `pkg/controller/controller_test.go`.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod parity_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
    use std::time::Duration;

    fn arc_fn<F>(f: F) -> ControllerFunc
    where
        F: Fn(CancelToken) -> anyhow::Result<()> + Send + Sync + 'static,
    {
        Arc::new(f)
    }

    /// Ported from `TestUpdateRemoveController`.
    #[test]
    fn test_update_remove_controller() {
        let mngr = Manager::new();
        mngr.update_controller("test", ControllerParams::default());
        assert!(mngr.remove_controller("test").is_ok());
    }

    /// Ported from `TestRemoveControllerNotFound`.
    #[test]
    fn test_remove_controller_not_found() {
        let mngr = Manager::new();
        let err = mngr.remove_controller("not-exists").unwrap_err();
        assert!(
            err.is::<ControllerNotFound>(),
            "expected ControllerNotFound, got: {}",
            err
        );
    }

    /// Ported from `TestRemoveControllerEmptyMap`.
    #[test]
    fn test_remove_controller_empty_map() {
        let mngr = Manager::default();
        let err = mngr.remove_controller("not-exists").unwrap_err();
        assert!(
            err.is::<ControllerMapEmpty>(),
            "expected ControllerMapEmpty, got: {}",
            err
        );
    }

    /// Ported from `TestRemoveControllerAndWaitNotFound`.
    #[test]
    fn test_remove_controller_and_wait_not_found() {
        let mngr = Manager::new();
        assert!(mngr.remove_controller_and_wait("not-exists").is_ok());
    }

    /// Ported from `TestRemoveControllerAndWaitEmptyMap`.
    #[test]
    fn test_remove_controller_and_wait_empty_map() {
        let mngr = Manager::default();
        let err = mngr.remove_controller_and_wait("not-exists").unwrap_err();
        assert!(
            err.is::<ControllerMapEmpty>(),
            "expected ControllerMapEmpty, got: {}",
            err
        );
    }

    /// Ported from `TestCreateController`.
    #[test]
    fn test_create_controller() {
        let iterations = Arc::new(AtomicU32::new(0));

        let mngr = Manager::new();

        let iter1 = Arc::clone(&iterations);
        let created = mngr.create_controller(
            "test",
            ControllerParams {
                do_func: Some(arc_fn(move |_token| {
                    iter1.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })),
                ..Default::default()
            },
        );
        assert!(created);

        // Second creation is a no-op.
        let iter2 = Arc::clone(&iterations);
        let created = mngr.create_controller(
            "test",
            ControllerParams {
                do_func: Some(arc_fn(move |_token| {
                    iter2.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })),
                ..Default::default()
            },
        );
        assert!(!created);

        assert!(mngr.remove_controller_and_wait("test").is_ok());
        assert_eq!(iterations.load(Ordering::SeqCst), 1);
    }

    /// Ported from `TestStopFunc`.
    #[test]
    fn test_stop_func() {
        let stop_func_ran = Arc::new(AtomicU32::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        let ran = Arc::clone(&stop_func_ran);
        let mngr = Manager::new();
        mngr.update_controller(
            "test",
            ControllerParams {
                run_interval: Duration::from_secs(1),
                do_func: Some(arc_fn(|_token| Ok(()))),
                stop_func: Some(arc_fn(move |_token| {
                    ran.store(1, Ordering::SeqCst);
                    let _ = tx.send(());
                    Ok(())
                })),
                ..Default::default()
            },
        );

        assert!(mngr.remove_controller("test").is_ok());

        // Wait up to 2 s for StopFunc to run.
        rx.recv_timeout(Duration::from_secs(2))
            .expect("StopFunc did not run within 2 s");
        assert_eq!(stop_func_ran.load(Ordering::SeqCst), 1);
    }

    /// Ported from `TestSelfExit`.
    #[test]
    fn test_self_exit() {
        let iterations = Arc::new(AtomicU32::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        let iter = Arc::clone(&iterations);
        let mngr = Manager::new();
        mngr.update_controller(
            "test",
            ControllerParams {
                run_interval: Duration::from_millis(100),
                do_func: Some(arc_fn(move |_token| {
                    iter.fetch_add(1, Ordering::SeqCst);
                    Err(new_exit_reason("test exit"))
                })),
                stop_func: Some(arc_fn({
                    let tx = tx.clone();
                    move |_token| {
                        let _ = tx.send(());
                        Ok(())
                    }
                })),
                ..Default::default()
            },
        );

        // The controller must NOT self-exit for 1 s.
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(()) => panic!("Controller exited unexpectedly"),
            Err(_) => {} // timeout — correct
        }
        assert_eq!(iterations.load(Ordering::SeqCst), 1);

        // Explicitly remove — now StopFunc must fire.
        let _ = mngr.remove_controller("test");
        rx.recv_timeout(Duration::from_secs(1))
            .expect("Controller did not exit after removal");
    }

    /// Ported from `TestRemoveAll`.
    #[test]
    fn test_remove_all() {
        let mngr = Manager::new();
        mngr.update_controller("test1", ControllerParams::default());
        mngr.update_controller("test2", ControllerParams::default());
        mngr.update_controller("test3", ControllerParams::default());
        mngr.update_controller("test1", ControllerParams::default());
        mngr.update_controller("test2", ControllerParams::default());
        mngr.update_controller("test3", ControllerParams::default());
        mngr.remove_all();
    }

    /// Ported from `TestRunController`.
    #[test]
    fn test_run_controller() {
        let mngr = Manager::new();
        let cnt = Arc::new(AtomicU32::new(0));

        let cnt2 = Arc::clone(&cnt);
        let ctrl = mngr.update_controller(
            "test",
            ControllerParams {
                do_func: Some(arc_fn(move |_token| {
                    if cnt2.load(Ordering::SeqCst) >= 2 {
                        return Ok(());
                    }
                    cnt2.fetch_add(1, Ordering::SeqCst);
                    Err(anyhow::anyhow!("temporary error"))
                })),
                run_interval: Duration::from_millis(1),
                error_retry_base_duration: Duration::from_millis(1),
                ..Default::default()
            },
        );

        // Wait until at least 2 successes.
        for n in 0..=100 {
            if ctrl.get_success_count() >= 2 {
                break;
            }
            if n == 100 {
                panic!(
                    "timeout waiting for controller to succeed, last error: {:?}",
                    ctrl.get_last_error()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // GetGlobalStatus must be non-nil (our impl returns Vec which is always non-nil).
        let _ = get_global_status();
        assert!(ctrl.get_success_count() > 0);
        assert_eq!(ctrl.get_failure_count(), 2);
        assert!(ctrl.get_last_error().is_ok());
        assert!(mngr.remove_controller("test").is_ok());
    }

    /// Ported from `TestCancellation`.
    #[test]
    fn test_cancellation() {
        let mngr = Manager::new();

        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (cancelled_tx, cancelled_rx) = std::sync::mpsc::channel::<()>();

        mngr.update_controller(
            "test",
            ControllerParams {
                do_func: Some(arc_fn(move |token| {
                    let _ = started_tx.send(());
                    token.wait_for_cancel();
                    let _ = cancelled_tx.send(());
                    Ok(())
                })),
                ..Default::default()
            },
        );

        // Wait for DoFunc to start.
        started_rx
            .recv_timeout(Duration::from_secs(60))
            .expect("timeout waiting for controller to start");

        mngr.remove_all();

        // Wait for cancellation.
        cancelled_rx
            .recv_timeout(Duration::from_secs(60))
            .expect("timeout waiting for controller to be cancelled");
    }

    /// Ported from `TestWaitForTermination`.
    #[test]
    fn test_wait_for_termination() {
        let mngr = Manager::new();
        mngr.update_controller("test1", ControllerParams::default());
        mngr.update_controller("test1", ControllerParams::default());

        // The controller must still be running (not yet terminated).
        // Poll for up to 20 ms and confirm it stays alive.
        let deadline = std::time::Instant::now() + Duration::from_millis(20);
        let mut terminated_early = false;
        while std::time::Instant::now() < deadline {
            let is_terminated = {
                let controllers = mngr.controllers.read().unwrap();
                if let Some(map) = controllers.as_ref() {
                    if let Some(ctrl) = map.get("test1") {
                        let (lock, _cv) = &*ctrl.terminated;
                        *lock.lock().unwrap()
                    } else {
                        true // already removed
                    }
                } else {
                    true
                }
            };
            if is_terminated {
                terminated_early = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            !terminated_early,
            "Controller terminated before being removed"
        );

        assert!(mngr.remove_controller_and_wait("test1").is_ok());
        // Confirmed by the AndWait blocking above — controller has terminated.
    }

    /// Ported from `TestConcurrentControllerUpdate`.
    #[test]
    fn test_concurrent_controller_update() {
        let result = Arc::new(AtomicI32::new(-1));
        let wait_to_execute = Arc::new((Mutex::new(false), Condvar::new()));

        let (start0_tx, start0_rx) = std::sync::mpsc::channel::<()>();
        let (complete0_tx, complete0_rx) = std::sync::mpsc::channel::<()>();
        let (complete2_tx, complete2_rx) = std::sync::mpsc::channel::<()>();
        // complete1 channel — we only need to detect if func1 completes before func2.
        let (complete1_tx, complete1_rx) = std::sync::mpsc::channel::<()>();

        let mngr = Manager::new();

        // func0: blocks until wait_to_execute is released.
        let res0 = Arc::clone(&result);
        let wte0 = Arc::clone(&wait_to_execute);
        mngr.update_controller(
            "test",
            ControllerParams {
                do_func: Some(arc_fn(move |_token| {
                    let _ = start0_tx.send(());
                    let (lock, cv) = &*wte0;
                    let guard = lock.lock().unwrap();
                    let _guard = cv.wait_while(guard, |released| !*released).unwrap();
                    res0.store(0, Ordering::SeqCst);
                    let _ = complete0_tx.send(());
                    Ok(())
                })),
                ..Default::default()
            },
        );

        // Wait for func0 to start.
        start0_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("timeout waiting for func0 to start");

        // Apply subsequent updates while func0 is blocked.
        let res1 = Arc::clone(&result);
        mngr.update_controller(
            "test",
            ControllerParams {
                do_func: Some(arc_fn(move |_token| {
                    res1.store(1, Ordering::SeqCst);
                    let _ = complete1_tx.send(());
                    Ok(())
                })),
                ..Default::default()
            },
        );

        let res2 = Arc::clone(&result);
        mngr.update_controller(
            "test",
            ControllerParams {
                do_func: Some(arc_fn(move |_token| {
                    res2.store(2, Ordering::SeqCst);
                    let _ = complete2_tx.send(());
                    Ok(())
                })),
                ..Default::default()
            },
        );

        // Release func0.
        {
            let (lock, cv) = &*wait_to_execute;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }
        complete0_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("timeout waiting for func0 to complete");

        // Either func1 or func2 must complete — the intermediate update may be elided.
        // func2 (the last update) must eventually run.
        select_recv(&complete2_rx, &complete1_rx, Duration::from_secs(30));

        assert!(mngr.remove_controller_and_wait("test").is_ok());
        assert_eq!(result.load(Ordering::SeqCst), 2);
    }
}

/// Helper: receive from chan1 or chan2 within timeout; panic if neither fires.
#[cfg(test)]
fn select_recv(
    chan1: &std::sync::mpsc::Receiver<()>,
    chan2: &std::sync::mpsc::Receiver<()>,
    timeout: Duration,
) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if chan1.try_recv().is_ok() || chan2.try_recv().is_ok() {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "Intermediate updates should have been elided — neither func1 nor func2 completed in time"
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_uses_shared_core_controller() {
        let scaffold = ControllerScaffold::scaffold();
        let config = scaffold.config();

        assert_eq!(config.name, "seriousum-controller");
        assert_eq!(config.group, "controller");
        assert_eq!(scaffold.status(), SharedControllerStatus::Stopped);
    }

    #[test]
    fn report_includes_rate_limit_when_present() {
        let scaffold = ControllerScaffold::new("worker")
            .with_group("dataplane")
            .with_rate_limit(Duration::from_secs(2));
        let report = scaffold.report();

        assert_eq!(report.name, "worker");
        assert_eq!(report.group, "dataplane");
        assert_eq!(report.rate_limit_ms, Some(2000));
        assert_eq!(report.status, SharedControllerStatus::Stopped);
        assert!(report.to_string().contains("rate_limit_ms=2000"));
    }

    #[tokio::test]
    async fn controller_delegates_worker_lifecycle() {
        let scaffold = ControllerScaffold::new("worker");
        scaffold.set_worker(|| async { Ok(()) }).await;
        scaffold.run_once().await.expect("run controller once");
        assert_eq!(scaffold.status(), SharedControllerStatus::Running);
        scaffold.stop().await;
        assert_eq!(scaffold.status(), SharedControllerStatus::Stopped);
    }

    #[test]
    fn run_returns_summary() {
        let output = run().expect("run controller scaffold");

        assert!(output.contains("controller scaffold ready"));
        assert!(output.contains("name=seriousum-controller"));
    }
}
