//! The single composition root. The executable and the acceptance tests both call
//! [`compose`], so there is never a second, test-only wiring of storage and engine.

use std::io::Read;
use std::sync::Arc;

use async_trait::async_trait;
use tracing::warn;

use convex_engine::ports::config::EngineConfig;
use convex_engine::ports::market_data::{MarketDataProvider, QuoteSource};
use convex_engine::ports::reference_data::ReferenceDataProvider;
use convex_engine::ports::runtime::{PublishedValuation, ResultPublisher};
use convex_engine::ports::snapshot::{MarketSnapshot, SnapshotRepository};
use convex_engine::ports::TraitError;
use convex_engine::{
    PricingEngine, PricingEngineBuilder, RuntimeInput, RuntimeMode, RuntimePlan, RuntimeServices,
};
use convex_ext_file::{
    EmptyCurveInputSource, EmptyEtfHoldingsSource, EmptyEtfQuoteSource, EmptyFxRateSource,
    EmptyIndexFixingSource, EmptyInflationFixingSource, EmptyIssuerReferenceSource,
    EmptyQuoteSource, EmptyRatingSource, EmptyVolatilitySource,
};

use crate::market::QuoteIngress;
use crate::{ConfigError, Server, ServerConfig, ServerProfile, StartupError};

const MAX_PLAN_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 1024 * 1024 * 1024;

type Cause = Box<dyn std::error::Error + Send + Sync>;

/// A composed, not yet started server. Call `engine.start()`, then serve.
pub struct Composition {
    /// HTTP/WebSocket server over the composed engine, access control and core workflow.
    pub server: Server,
    /// The composed engine; its runtime lifecycle has not started.
    pub engine: Arc<PricingEngine>,
}

/// Validate startup settings, open storage and compose the engine and server.
///
/// Nothing is started or bound. The caller owns the lifecycle: `engine.start()`,
/// then `server.start()` or `server.serve_until(..)`. Dropping a stopped
/// composition releases the storage file, so a second call on the same
/// `storage_path` reopens the same durable state.
pub async fn compose(config: ServerConfig) -> Result<Composition, StartupError> {
    config.validate_startup()?;
    let access = crate::access::AccessControl::from_settings(&config.access).await?;
    if access.is_local_development() {
        warn!("Explicit local development access: loopback API has no authentication");
    }
    if config.profile == ServerProfile::Demo {
        warn!("Demo profile enabled: all server output is demonstration data");
    }
    let plan = match config.profile {
        ServerProfile::Replay | ServerProfile::Live => Some(read_plan(&config)?),
        ServerProfile::Demo | ServerProfile::Production => None,
    };

    let (storage, runtime_store, core_fixings) =
        convex_ext_redb::create_redb_storage_with_core(&config.storage_path)
            .map_err(stage("storage"))?;
    let storage = Arc::new(storage);
    // The core workflow's file snapshots need Unix directory fsync.
    let core = if cfg!(unix) {
        Some(Arc::new(convex_engine::core_workflow::CoreWorkflow::new(
            storage.bonds.clone(),
            storage.curves.clone(),
            core_fixings,
            Arc::new(
                convex_ext_file::FileSnapshotRepository::new(format!(
                    "{}.core-snapshots",
                    config.storage_path
                ))
                .map_err(stage("core_snapshots"))?,
            ),
            runtime_store.clone(),
            core_build_id()?,
        )))
    } else {
        None
    };

    // Live quotes arrive only through the HTTP ingress, scoped to the plan's quote keys.
    let ingress = plan
        .as_ref()
        .filter(|plan| plan.mode == RuntimeMode::Live)
        .map(|plan| {
            Arc::new(QuoteIngress::new(
                plan.assembly.quotes.iter().map(|q| q.key.clone()),
            ))
        });
    let quotes: Arc<dyn QuoteSource> = match &ingress {
        Some(ingress) => ingress.clone(),
        None => Arc::new(EmptyQuoteSource),
    };
    // Unused auxiliary product ports are explicit; replay reads the selected canonical event source.
    let market_data = MarketDataProvider {
        quotes,
        curve_inputs: Arc::new(EmptyCurveInputSource),
        index_fixings: Arc::new(EmptyIndexFixingSource),
        volatility: Arc::new(EmptyVolatilitySource),
        fx_rates: Arc::new(EmptyFxRateSource),
        inflation_fixings: Arc::new(EmptyInflationFixingSource),
        etf_quotes: Arc::new(EmptyEtfQuoteSource),
    };
    let reference_data = ReferenceDataProvider {
        issuers: Arc::new(EmptyIssuerReferenceSource),
        ratings: Arc::new(EmptyRatingSource),
        etf_holdings: Arc::new(EmptyEtfHoldingsSource),
    };

    let mut builder = PricingEngineBuilder::new()
        .with_config(EngineConfig::default())
        .with_market_data(Arc::new(market_data))
        .with_reference_data(Arc::new(reference_data))
        .with_storage(storage.clone());
    builder = match plan {
        Some(plan) => with_runtime(builder, &config, plan, &storage, runtime_store).await?,
        None => builder.with_demo_mode(),
    };
    let engine = Arc::new(builder.build()?);

    let mut server = Server::with_access_control(config, engine.clone(), access);
    if let Some(core) = core {
        server = server.with_core_workflow(core);
    }
    if let Some(ingress) = ingress {
        server = server.with_quote_ingress(ingress);
    }
    Ok(Composition { server, engine })
}

/// The bounded plan file, in the mode its profile names, for this executable.
fn read_plan(config: &ServerConfig) -> Result<RuntimePlan, StartupError> {
    let path = config
        .runtime_plan
        .as_ref()
        .ok_or(ConfigError::InvalidValue {
            field: "runtime_plan",
            expected: "bounded JSON runtime plan",
        })?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_PLAN_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PLAN_BYTES {
        return Err(convex_engine::EngineError::from(
            convex_engine::RuntimeError::CapacityExceeded,
        )
        .into());
    }
    let plan: RuntimePlan =
        serde_json::from_slice(&bytes).map_err(|_| ConfigError::InvalidValue {
            field: "runtime_plan",
            expected: "complete supported schema 1 plan",
        })?;
    let mode = match config.profile {
        ServerProfile::Live => RuntimeMode::Live,
        _ => RuntimeMode::Replay,
    };
    if plan.mode != mode || plan.build_id.as_str() != crate::SERVER_BUILD_ID {
        return Err(
            convex_engine::EngineError::from(convex_engine::RuntimeError::Configuration(
                "runtime plan must select the profile mode and this executable build identity",
            ))
            .into(),
        );
    }
    Ok(plan)
}

async fn with_runtime(
    builder: PricingEngineBuilder,
    config: &ServerConfig,
    plan: RuntimePlan,
    storage: &convex_engine::ports::storage::StorageAdapter,
    runtime_store: Arc<convex_ext_redb::RedbRuntimeStore>,
) -> Result<PricingEngineBuilder, StartupError> {
    let live = plan.mode == RuntimeMode::Live;
    let snapshots: Arc<dyn SnapshotRepository> = if live {
        Arc::new(UnstoredSnapshots)
    } else {
        Arc::new(
            convex_ext_file::FileSnapshotRepository::new(format!(
                "{}.snapshots",
                config.storage_path
            ))
            .map_err(stage("runtime_snapshots"))?,
        )
    };
    let fixings = Arc::new(convex_ext_file::JsonFixingRepository::empty());
    let sources = plan
        .assembly
        .quotes
        .iter()
        .map(|q| convex_core::contracts::SourceId::new(&q.key.source))
        .collect::<Result<Vec<_>, _>>()
        .map_err(stage("runtime_plan_sources"))?;
    let capabilities = if live {
        crate::configuration_admin::local_live_capabilities(sources)
    } else {
        crate::configuration_admin::local_replay_capabilities(sources)
    };
    let cases = Arc::new(convex_engine::configuration::ConfigurationUseCases::new(
        storage.configuration.clone(),
        storage.bonds.clone(),
        storage.configs.clone(),
        fixings.clone(),
        snapshots.clone(),
        capabilities.map_err(stage("configuration_capabilities"))?,
    ));
    let mut builder = builder;
    if let Some(active) = cases
        .active(&plan.assembly.configuration.id)
        .await
        .map_err(stage("active_configuration"))?
    {
        builder = builder.with_config(EngineConfig {
            max_concurrent_calcs: active.staged.document.process.max_concurrent_calcs,
            ..EngineConfig::default()
        });
    }
    let (publisher, input): (Arc<dyn ResultPublisher>, RuntimeInput) = if live {
        (Arc::new(DisplayOnlyPublisher), RuntimeInput::Live)
    } else {
        let quotes = config
            .quotes_file
            .as_ref()
            .ok_or(ConfigError::InvalidValue {
                field: "quotes_file",
                expected: "canonical JSONL quote events",
            })?;
        let publisher = convex_ext_file::JsonlResultPublisher::new(format!(
            "{}.results.jsonl",
            config.storage_path
        ))
        .map_err(stage("result_publisher"))?;
        let replay =
            convex_ext_file::FileQuoteReplay::open(quotes).map_err(stage("quote_replay"))?;
        (Arc::new(publisher), RuntimeInput::Replay(Box::new(replay)))
    };
    Ok(builder
        .with_configuration_use_cases(cases)
        .with_runtime(RuntimeServices {
            plan,
            snapshots,
            fixings,
            store: runtime_store,
            publisher,
            input,
        }))
}

/// Live market snapshots are not stored. The Live runtime never publishes one, and
/// configuration validation only resolves snapshots, so nothing is ever found here.
struct UnstoredSnapshots;

#[async_trait]
impl SnapshotRepository for UnstoredSnapshots {
    async fn publish(&self, _: &MarketSnapshot) -> Result<(), TraitError> {
        Err(TraitError::Unsupported(
            "live market snapshots are not stored",
        ))
    }

    async fn get(
        &self,
        _: &convex_core::contracts::SnapshotId,
    ) -> Result<Option<MarketSnapshot>, TraitError> {
        Ok(None)
    }
}

/// Live results reach WebSocket subscribers and `/api/v1/runtime/results` only. They are
/// not archived: the JSONL archive's 100,000 delivery-key bound would fill within about
/// 400 ticks of a 256-instrument universe.
struct DisplayOnlyPublisher;

#[async_trait]
impl ResultPublisher for DisplayOnlyPublisher {
    async fn publish(&self, _: &PublishedValuation) -> Result<(), TraitError> {
        Ok(())
    }
}

fn stage<E: Into<Cause>>(stage: &'static str) -> impl FnOnce(E) -> StartupError {
    move |error| StartupError::Composition {
        stage,
        source: error.into(),
    }
}

// Cache identities change with the actual executable, including local builds.
fn core_build_id() -> Result<convex_core::contracts::ModelId, StartupError> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(std::env::current_exe()?)?;
    if file.metadata()?.len() > MAX_EXECUTABLE_BYTES {
        return Err(stage("build_identity")(
            "executable exceeds core build identity bound",
        ));
    }
    let mut hash = Sha256::new();
    let mut chunk = [0u8; 65536];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        hash.update(&chunk[..count]);
    }
    convex_core::contracts::ModelId::new(format!("sha256:{:x}", hash.finalize()))
        .map_err(stage("build_identity"))
}
