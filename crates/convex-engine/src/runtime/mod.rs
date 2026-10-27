//! One bounded event-driven pricing lifecycle. See `RUNTIME.md` for acknowledgment and recovery.
#![forbid(unsafe_code)]

mod actor;
mod worker;
use crate::{
    context::NumericalSettings,
    snapshot::{SnapshotAssembler, SnapshotAssembly},
    CalculationGraph, EngineError,
};
use actor::Actor;
use convex_core::contracts::*;
use convex_ports::{
    fixing::FixingRepository,
    market_data::{QuoteEventSource, QuoteSource, StreamError, StreamReceiver},
    quote_event::QuoteEvent,
    runtime::{
        PublishedValuation, ResultPublisher, RuntimeCheckpoint, RuntimeStore, MAX_RUNTIME_EVENTS,
    },
    snapshot::{content_digest, MarketSnapshot, SnapshotRepository},
    TraitError,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, oneshot, watch, Mutex};

pub use convex_ports::configuration::RuntimeMode;

/// A bounded fixed-bullet universe over one exact quantitative configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePlan {
    /// Application format.
    pub schema_version: u32,
    /// Durable checkpoint/publication namespace.
    pub runtime_id: SourceId,
    /// Explicit data-time profile.
    pub mode: RuntimeMode,
    /// Exact initial reference/configuration/curve pins and quote requirements.
    pub assembly: SnapshotAssembly,
    /// Initial captured trace. Replay obtains its trace only from its source; Live has none.
    pub seed_events: Vec<QuoteEvent>,
    /// Explicit settlement; change requires a new runtime identity/plan.
    pub settlement_date: BusinessDate,
    /// Composition-root build identity, retained in every context/result.
    pub build_id: ModelId,
    /// Supported fixed-bullet mid measures; unsupported measures fail setup.
    pub measures: Vec<MeasureId>,
    /// Existing compiled numerical policy and bump settings.
    pub numerical_settings: NumericalSettings,
}
impl RuntimePlan {
    /// Validate the operational slice without substituting terms, curves, units or models.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        crate::calibration_replay::rebuild_captured_curves(
            &self.assembly.curve_recipes,
            self.assembly.calibration.as_ref(),
            self.assembly.as_of,
            self.assembly.known_at,
        )
        .map_err(TraitError::from)?;
        if self.schema_version != 1
            || self.assembly.generation.get() != 1
            || self.assembly.instruments.is_empty()
            || self.assembly.instruments.len() > 256
            || self.assembly.quotes.len() != self.assembly.instruments.len()
            || self.seed_events.len() > MAX_RUNTIME_EVENTS
            || self.measures.is_empty()
            || !self.assembly.fixings.is_empty()
            || self.numerical_settings.policy_id.as_str() != crate::snapshot::BOND_NUMERICAL_POLICY
            || self.numerical_settings.bump_bps.get() <= 0.0
            || self.numerical_settings.bump_bps.get() > 10000.0
            // Replay reads its trace from the source; Live takes market data only from its subscription.
            || (self.mode != RuntimeMode::Captured && !self.seed_events.is_empty())
        {
            return Err(RuntimeError::Configuration(
                "invalid/unsupported runtime plan",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for pin in &self.assembly.instruments {
            if !ids.insert(&pin.instrument_id)
                || self
                    .assembly
                    .quotes
                    .iter()
                    .filter(|q| q.key.instrument_id.as_str() == pin.instrument_id.as_str())
                    .count()
                    != 1
            {
                return Err(RuntimeError::Configuration(
                    "one unique reference and selected quote source per instrument are required",
                ));
            }
        }
        // Each measure is published by at least one product model; duplicates are refused.
        let mut measures = std::collections::BTreeSet::new();
        for measure in &self.measures {
            if !measures.insert(measure)
                || !crate::optional_valuation::PRODUCT_MODELS
                    .iter()
                    .any(|model| {
                        crate::optional_valuation::product_measures(model)
                            .contains(&measure.as_str())
                    })
            {
                return Err(RuntimeError::Configuration(
                    "unsupported/duplicate runtime measure",
                ));
            }
        }
        for event in &self.seed_events {
            event.validate()?;
        }
        Ok(())
    }
    /// Construct the exact context used by runtime workers for this declared instrument.
    pub fn context(
        &self,
        snapshot: &MarketSnapshot,
        instrument: &InstrumentId,
    ) -> Result<crate::context::ValuationContext, RuntimeError> {
        self.validate()?;
        worker::context(self, snapshot, instrument)
    }
    fn digest(&self) -> Result<String, RuntimeError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| RuntimeError::Configuration("plan serialization failed"))?;
        if bytes.len() > convex_ports::snapshot::MAX_SNAPSHOT_BYTES {
            return Err(RuntimeError::CapacityExceeded);
        }
        Ok(content_digest(b"convex.runtime-plan.v1\0", &bytes))
    }
}
/// Genuine data-input boundary; source ownership is transferred to the lifecycle exactly once.
pub enum RuntimeInput {
    /// Bounded, acknowledged `PricingEngine::submit_update` calls.
    Captured,
    /// Pull-based canonical source, such as `FileQuoteReplay`.
    Replay(Box<dyn QuoteEventSource>),
    /// Subscribed quote source, such as the server's HTTP quote ingress.
    Live(Arc<dyn QuoteSource>),
}
/// Runtime I/O dependencies. All pricing dependencies are resolved before CPU work begins.
pub struct RuntimeServices {
    /// Exact declared operating universe.
    pub plan: RuntimePlan,
    /// Immutable content-addressed input storage.
    pub snapshots: Arc<dyn SnapshotRepository>,
    /// Publication-aware fixings; this fixed-bullet slice requires no fixing keys.
    pub fixings: Arc<dyn FixingRepository>,
    /// Durable accepted-work and result/outbox storage.
    pub store: Arc<dyn RuntimeStore>,
    /// Canonical publication; failure remains undelivered in the checkpoint.
    pub publisher: Arc<dyn ResultPublisher>,
    /// Owned input path.
    pub input: RuntimeInput,
}
/// Setup refusal when no plan measure is published by an instrument's own model.
pub(crate) const NO_APPLICABLE_MEASURE: &str =
    "no requested measure applies to this instrument's product";
/// Lifecycle failure classes contain no provider credentials or raw records.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RuntimeError {
    /// Invalid/unsupported configured capability.
    #[error("RUNTIME_CONFIGURATION: {0}")]
    Configuration(&'static str),
    /// Lifecycle does not permit the operation.
    #[error("RUNTIME_STATE: {0}")]
    State(&'static str),
    /// Bounded admission or retained state is full.
    #[error("RUNTIME_CAPACITY_EXCEEDED")]
    CapacityExceeded,
    /// Durable acceptance/recovery could not be verified.
    #[error("RUNTIME_STORAGE_UNAVAILABLE")]
    Storage,
    /// A published input/result has unsupported or corrupt lineage.
    #[error("RUNTIME_RECOVERY_CONFLICT")]
    RecoveryConflict,
    /// Output acknowledgment failed; retry the unchanged outbox after restart.
    #[error("RUNTIME_PUBLICATION_UNAVAILABLE")]
    Publication,
    /// Bounded I/O budget expired. Accepted work remains checkpointed.
    #[error("RUNTIME_DEADLINE_EXCEEDED")]
    Deadline,
    /// Worker join or graph commit failed.
    #[error("RUNTIME_WORKER_FAILED")]
    Worker,
    /// Live delivery lost continuity, or replay decode/prefix verification failed.
    #[error("RUNTIME_SOURCE_FAILED")]
    Source,
}
impl From<TraitError> for RuntimeError {
    fn from(value: TraitError) -> Self {
        match value {
            TraitError::CapacityExceeded => Self::CapacityExceeded,
            TraitError::Conflict(_) | TraitError::InvalidInput(_) | TraitError::Unsupported(_) => {
                Self::RecoveryConflict
            }
            _ => Self::Storage,
        }
    }
}
/// Explicit lifecycle phase; liveness, dependency readiness and data quality are distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePhase {
    /// No lifecycle started.
    New,
    /// Verifying exact dependencies and restoring work.
    Starting,
    /// Dependencies restored, current context valid and outbox acknowledged.
    Ready,
    /// Dependencies restored, but required data is missing/nonvalid or delivery was lost.
    Degraded,
    /// Admission closed; draining accepted work and joined workers.
    Draining,
    /// All owned tasks have joined.
    Stopped,
    /// A typed failure stopped acceptance; checkpoint remains recoverable.
    Failed,
    /// Explicit idle demo with no production calculations.
    Demo,
}
/// Low-cardinality operator status. This is never strategy/execution readiness.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeHealth {
    /// Lifecycle state.
    pub phase: RuntimePhase,
    /// Only valid configured runtime data satisfies analytics readiness.
    pub ready: bool,
    /// Explicit operating data-time profile, absent in idle demo/unconfigured state.
    pub mode: Option<RuntimeMode>,
    /// Latest frozen snapshot data condition.
    pub quality: DataQuality,
    /// Published input identity, when available.
    pub snapshot_id: Option<SnapshotId>,
    /// Restored/accepted target generation.
    pub generation: u64,
    /// Durable result revision; never resets on restart.
    pub revision: u64,
    /// Number of undelivered result records.
    pub pending_publications: usize,
    /// Finite replay reached its end. Historical context remains inspectable.
    pub replay_complete: bool,
    /// Stable failure classification without source text.
    pub failure: Option<String>,
    /// Configuration identity of the running plan.
    pub configuration_id: Option<SourceId>,
    /// Exact content version accepted by current work.
    pub configuration_version: Option<Version>,
    /// Active-pointer audit sequence accepted by current work, absent for older explicit plans.
    pub activation_revision: Option<Version>,
    /// Most recently requested durable selection; a mismatch closes readiness during application.
    pub desired_activation_revision: Option<Version>,
}
impl Default for RuntimeHealth {
    fn default() -> Self {
        Self {
            phase: RuntimePhase::New,
            ready: false,
            mode: None,
            quality: DataQuality::Invalid,
            snapshot_id: None,
            generation: 0,
            revision: 0,
            pending_publications: 0,
            replay_complete: false,
            failure: None,
            configuration_id: None,
            configuration_version: None,
            activation_revision: None,
            desired_activation_revision: None,
        }
    }
}
/// Durable acceptance receipt; work may still be calculating after this is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedUpdate {
    /// CAS revision proving accepted input persistence.
    pub checkpoint_revision: u64,
    /// Captured next generation.
    pub generation: Version,
    /// Atomic active selection pinned by this accepted work.
    pub activation_revision: Option<Version>,
}
struct QuoteCommand {
    event: QuoteEvent,
    as_of: UtcTimestamp,
    known_at: UtcTimestamp,
    reply: oneshot::Sender<Result<AcceptedUpdate, RuntimeError>>,
}
enum Command {
    Quote(QuoteCommand),
    RefreshConfiguration {
        at: UtcTimestamp,
        reply: oneshot::Sender<Result<AcceptedUpdate, RuntimeError>>,
    },
}
struct Lifecycle {
    services: Option<RuntimeServices>,
    commands: Option<mpsc::Sender<Command>>,
    stop: Option<watch::Sender<bool>>,
    task: Option<tokio::task::JoinHandle<Result<(), RuntimeError>>>,
    interactive_jobs: Vec<tokio::task::JoinHandle<()>>,
}
pub(crate) struct RuntimeController {
    lifecycle: Mutex<Lifecycle>,
    health: Arc<parking_lot::RwLock<RuntimeHealth>>,
    results: Arc<parking_lot::RwLock<Arc<[PublishedValuation]>>>,
    updates: broadcast::Sender<PublishedValuation>,
    demo: bool,
    stopped: watch::Sender<bool>,
    build_id: Option<ModelId>,
    settlement_date: Option<BusinessDate>,
    configuration_id: Option<SourceId>,
    interactive_slots: Arc<tokio::sync::Semaphore>,
}
impl RuntimeController {
    pub(crate) fn new(services: Option<RuntimeServices>, demo: bool, concurrency: usize) -> Self {
        let build_id = services.as_ref().map(|s| s.plan.build_id.clone());
        let settlement_date = services.as_ref().map(|s| s.plan.settlement_date);
        let configuration_id = services
            .as_ref()
            .map(|s| s.plan.assembly.configuration.id.clone());
        let (updates, _) = broadcast::channel(256);
        let (stopped, _) = watch::channel(false);
        Self {
            lifecycle: Mutex::new(Lifecycle {
                services,
                commands: None,
                stop: None,
                task: None,
                interactive_jobs: Vec::new(),
            }),
            health: Arc::new(parking_lot::RwLock::new(RuntimeHealth::default())),
            results: Arc::new(parking_lot::RwLock::new(Arc::default())),
            updates,
            demo,
            stopped,
            build_id,
            settlement_date,
            configuration_id,
            interactive_slots: Arc::new(tokio::sync::Semaphore::new(
                if (1..=64).contains(&concurrency) {
                    concurrency
                } else {
                    0
                },
            )),
        }
    }
    pub(crate) async fn evaluate_context(
        &self,
        snapshot: MarketSnapshot,
        context: crate::context::ValuationContext,
        request: RequestId,
        calculated: UtcTimestamp,
    ) -> Result<crate::snapshot::SnapshotValuationResult, TraitError> {
        if self.build_id.as_ref() != Some(&context.spec().build_id) {
            return Err(TraitError::Unsupported(
                "requested build is not configured in this runtime",
            ));
        }
        context.validate_snapshot(&snapshot)?;
        let mut lifecycle = self.lifecycle.lock().await;
        if !matches!(
            self.health.read().phase,
            RuntimePhase::Ready | RuntimePhase::Degraded
        ) {
            return Err(TraitError::SourceNotAvailable(
                "runtime context admission is closed".into(),
            ));
        }
        let permit = self
            .interactive_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| TraitError::CapacityExceeded)?;
        lifecycle.interactive_jobs.retain(|job| !job.is_finished());
        let (completed, received) = oneshot::channel();
        lifecycle
            .interactive_jobs
            .push(tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let result =
                    crate::snapshot::SnapshotValuator::new(context.spec().build_id.clone(), 1)
                        .and_then(|valuator| {
                            // A supplied context is valued only on Valid inputs.
                            worker::value(
                                &valuator,
                                worker::Input::Supplied(&snapshot),
                                &context,
                                request,
                                calculated,
                                false,
                            )
                            .map_err(|_| {
                                TraitError::InvalidInput("invalid supplied runtime context")
                            })
                        });
                // A canceled HTTP request abandons its reply, not ownership of the bounded CPU job.
                let _ = completed.send(result);
            }));
        drop(lifecycle);
        received
            .await
            .map_err(|_| TraitError::SourceNotAvailable("runtime context worker failed".into()))?
    }
    pub(crate) fn subscribe_shutdown(&self) -> watch::Receiver<bool> {
        self.stopped.subscribe()
    }
    pub(crate) fn health(&self) -> RuntimeHealth {
        self.health.read().clone()
    }
    pub(crate) fn results(&self) -> Vec<PublishedValuation> {
        self.results.read().to_vec()
    }
    pub(crate) fn settlement_date(&self) -> Option<BusinessDate> {
        self.settlement_date
    }
    pub(crate) fn configuration_id(&self) -> Option<&SourceId> {
        self.configuration_id.as_ref()
    }
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<PublishedValuation> {
        self.updates.subscribe()
    }
    pub(crate) async fn start(&self, engine: &crate::PricingEngine) -> Result<(), EngineError> {
        let mut lifecycle = self.lifecycle.lock().await;
        if self.health.read().phase != RuntimePhase::New {
            return Err(RuntimeError::State("start requires a new engine instance").into());
        }
        if self.demo {
            let mut health = self.health.write();
            health.phase = RuntimePhase::Demo;
            health.quality = DataQuality::Demo;
            return Ok(());
        }
        self.health.write().phase = RuntimePhase::Starting;
        let Some(services) = lifecycle.services.take() else {
            self.fail(&RuntimeError::Configuration(
                "runtime services required; demo must be explicit",
            ));
            return Err(RuntimeError::Configuration(
                "runtime services required; demo must be explicit",
            )
            .into());
        };
        let started = Actor::restore(
            services,
            engine,
            self.health.clone(),
            self.results.clone(),
            self.updates.clone(),
        )
        .await;
        let actor = match started {
            Ok(actor) => actor,
            Err(error) => {
                self.fail(&error);
                return Err(error.into());
            }
        };
        let (commands, rx) = mpsc::channel(128);
        let (stop, stop_rx) = watch::channel(false);
        lifecycle.commands = Some(commands);
        lifecycle.stop = Some(stop);
        lifecycle.task = Some(tokio::spawn(actor.run(rx, stop_rx)));
        Ok(())
    }
    fn fail(&self, error: &RuntimeError) {
        let mut health = self.health.write();
        health.phase = RuntimePhase::Failed;
        health.ready = false;
        health.failure = Some(error.to_string());
    }
    pub(crate) async fn submit(
        &self,
        event: QuoteEvent,
        as_of: UtcTimestamp,
        known_at: UtcTimestamp,
    ) -> Result<AcceptedUpdate, RuntimeError> {
        event.validate()?;
        let (reply, received) = oneshot::channel();
        {
            let lifecycle = self.lifecycle.lock().await;
            if !matches!(
                self.health.read().phase,
                RuntimePhase::Ready | RuntimePhase::Degraded
            ) || self.health.read().mode != Some(RuntimeMode::Captured)
            {
                return Err(RuntimeError::State("captured input admission is closed"));
            }
            let tx = lifecycle
                .commands
                .as_ref()
                .ok_or(RuntimeError::State("input admission is closed"))?;
            tx.try_send(Command::Quote(QuoteCommand {
                event,
                as_of,
                known_at,
                reply,
            }))
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => RuntimeError::CapacityExceeded,
                mpsc::error::TrySendError::Closed(_) => {
                    RuntimeError::State("input admission is closed")
                }
            })?;
        }
        received
            .await
            .map_err(|_| RuntimeError::State("input was not acknowledged; reconcile checkpoint"))?
    }
    pub(crate) async fn refresh_configuration(
        &self,
        record: &convex_ports::configuration::ActivationRecord,
    ) -> Result<AcceptedUpdate, RuntimeError> {
        let (reply, received) = oneshot::channel();
        {
            let lifecycle = self.lifecycle.lock().await;
            if !matches!(
                self.health.read().phase,
                RuntimePhase::Ready | RuntimePhase::Degraded
            ) {
                return Err(RuntimeError::State(
                    "configuration application requires an accepting runtime",
                ));
            }
            {
                let mut health = self.health.write();
                health.desired_activation_revision = Some(
                    health
                        .desired_activation_revision
                        .map_or(record.revision, |r| r.max(record.revision)),
                );
                if health.activation_revision != health.desired_activation_revision {
                    health.ready = false;
                    health.phase = RuntimePhase::Degraded;
                }
            }
            let tx = lifecycle
                .commands
                .as_ref()
                .ok_or(RuntimeError::State("configuration admission is closed"))?;
            tx.try_send(Command::RefreshConfiguration {
                at: record.command.activated_at,
                reply,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => RuntimeError::CapacityExceeded,
                mpsc::error::TrySendError::Closed(_) => {
                    RuntimeError::State("configuration admission is closed")
                }
            })?;
        }
        received.await.map_err(|_| {
            RuntimeError::State(
                "configuration application was not acknowledged; reconcile checkpoint",
            )
        })?
    }
    pub(crate) async fn shutdown(&self) -> Result<(), EngineError> {
        let mut lifecycle = self.lifecycle.lock().await;
        if self.health.read().phase == RuntimePhase::Stopped {
            return Ok(());
        }
        {
            let mut health = self.health.write();
            health.phase = RuntimePhase::Draining;
            health.ready = false;
        }
        lifecycle.commands.take();
        if let Some(stop) = lifecycle.stop.take() {
            let _ = stop.send(true);
        }
        let mut failure = None;
        if let Some(task) = lifecycle.task.take() {
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => failure = Some(error),
                Err(_) => failure = Some(RuntimeError::Worker),
            }
        }
        for job in lifecycle.interactive_jobs.drain(..) {
            if job.await.is_err() && failure.is_none() {
                failure = Some(RuntimeError::Worker);
            }
        }
        if let Some(error) = failure {
            self.fail(&error);
            self.stopped.send_replace(true);
            return Err(error.into());
        }
        self.health.write().phase = RuntimePhase::Stopped;
        self.stopped.send_replace(true);
        Ok(())
    }
}
impl Drop for RuntimeController {
    fn drop(&mut self) {
        // Drop is an emergency signal; callers must await shutdown to guarantee joining.
        if let Some(stop) = self.lifecycle.get_mut().stop.take() {
            let _ = stop.send(true);
        }
    }
}
const IO_BUDGET: Duration = Duration::from_secs(10);
async fn bounded<T>(
    future: impl std::future::Future<Output = Result<T, TraitError>>,
) -> Result<T, RuntimeError> {
    tokio::time::timeout(IO_BUDGET, future)
        .await
        .map_err(|_| RuntimeError::Deadline)?
        .map_err(Into::into)
}
