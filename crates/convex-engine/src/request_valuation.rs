//! Request-time valuation of callables and floating rate notes (G18, D57): one
//! unpublished snapshot of one stored instrument, with the request's mark as a labelled
//! `request` quote, valued by the runtime as the stream values a bond.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_core::contracts::{
    BusinessDate, CurveId, InstrumentId, MeasureId, ModelId, UtcTimestamp, ValuationSubject,
    Version,
};
use convex_ports::{
    configuration::{ConfigurationPin, InstrumentPin, NumericalSettings, SnapshotAssembly},
    fixing::FixingKey,
    quote_event::{
        FieldUpdate, QuoteChange, QuoteEvent, QuoteEventKind, QuoteField, QuoteKey, QuoteQuality,
        QuoteUnit, QuoteValue,
    },
    reference_repository::{ReferenceError, ReferenceRecord},
    snapshot::{MarketSnapshot, QuoteRequirement},
    storage::StorageAdapter,
    TraitError,
};
use rust_decimal::Decimal;

use crate::context::{ValuationContext, ValuationContextSpec, ValuationScenario};
use crate::curve_resolution::CurveResolutionError;
use crate::optional_valuation::{product_measures, product_model};
use crate::runtime::RuntimeServices;
use crate::snapshot::{read_curve_ids, SnapshotAssembler, SnapshotValuationResult};
use crate::RequestContext;

/// Quote source and session of a request's mark.
pub const REQUEST_SOURCE: &str = "request";

/// What the composed runtime fixes for every request it values: its measures, numerical
/// settings and build, and its plan's explicit fixings and source skew.
pub(crate) struct RequestValuer {
    assembler: SnapshotAssembler,
    measures: Vec<MeasureId>,
    numerical_settings: NumericalSettings,
    build_id: ModelId,
    fixings: Vec<FixingKey>,
    max_source_skew_ms: i64,
}

/// One request-time valuation and the unpublished snapshot it read.
pub struct RequestValuation {
    /// The runtime's valuation of the request context.
    pub result: SnapshotValuationResult,
    /// Every consumed (curve, version) pin, roots and dependencies.
    pub curves: Vec<(String, Version)>,
}

/// Why a request-time valuation was refused. Nothing is substituted.
#[derive(Debug, thiserror::Error)]
pub enum RequestValuationError {
    /// No runtime is composed, so nothing values callables and floating rate notes.
    #[error(
        "this server runs no runtime; callables and floating rate notes are valued through one"
    )]
    NoRuntime,
    /// Inline terms that are not the stored terms at the cutoff (D62).
    #[error("callables and floating rate notes price stored terms: store these terms, or send the ones stored at the cutoff")]
    UnstoredTerms,
    /// The stored reference could not be read.
    #[error(transparent)]
    Reference(#[from] ReferenceError),
    /// A curve the valuation reads is missing, stale or does not fit its role.
    #[error(transparent)]
    Curves(#[from] CurveResolutionError),
    /// Assembly, admission or valuation failed.
    #[error(transparent)]
    Port(#[from] TraitError),
}

impl RequestValuer {
    /// The runtime's settings; references and configurations come from `storage`.
    pub(crate) fn new(services: &RuntimeServices, storage: &StorageAdapter) -> Self {
        let plan = &services.plan;
        Self {
            assembler: SnapshotAssembler::new(
                storage.bonds.clone(),
                storage.configs.clone(),
                services.fixings.clone(),
                services.snapshots.clone(),
            ),
            measures: plan.measures.clone(),
            numerical_settings: plan.numerical_settings.clone(),
            build_id: plan.build_id.clone(),
            fixings: plan.assembly.fixings.clone(),
            max_source_skew_ms: plan.assembly.max_source_skew_ms,
        }
    }

    /// The one-instrument snapshot of `reference` at `known_at` and its context. The
    /// curves are the ones the product's valuation reads, read through `context` at its
    /// cutoff (D69). A missing or stale one refuses the request as it does for bullets
    /// (P9a); a curve the product does not read refuses nothing.
    pub(crate) async fn prepare(
        &self,
        context: &mut RequestContext,
        reference: &ReferenceRecord,
        settlement_date: BusinessDate,
        known_at: UtcTimestamp,
        mark: Option<Decimal>,
    ) -> Result<(MarketSnapshot, ValuationContext, Vec<(String, Version)>), RequestValuationError>
    {
        let bond = reference
            .bond
            .as_ref()
            .ok_or(TraitError::InvalidInput("the stored reference is deleted"))?;
        // The context reads its curves at its own cutoff, so the snapshot must share it.
        if known_at != context.known_at {
            return Err(
                TraitError::InvalidInput("a request values at its context's cutoff").into(),
            );
        }
        let config = context.config();
        if !config.applies_to.matches(bond) {
            return Err(CurveResolutionError::NotCovered(context.id.clone()).into());
        }
        let model = product_model(bond);
        // The runtime measure set for the product, as `worker::context` selects it.
        let measures: Vec<_> = self
            .measures
            .iter()
            .filter(|measure| product_measures(model).contains(&measure.as_str()))
            .cloned()
            .collect();
        if measures.is_empty() {
            return Err(TraitError::Unsupported(
                "the runtime requests no measure this product publishes",
            )
            .into());
        }
        let roots: Vec<_> = read_curve_ids(model, &measures, &config.analytics_curves)
            .into_iter()
            .map(convex_core::ids::CurveId::new)
            .collect();
        let invalid = |_| TraitError::InvalidInput("invalid request identity");
        let configuration = ConfigurationPin {
            id: context.id.clone(),
            version: Version::new(config.version).map_err(invalid)?,
        };
        let curves = context.read(&roots).await?;
        let key = QuoteKey {
            instrument_id: reference.instrument_id.clone(),
            source: REQUEST_SOURCE.into(),
        };
        let instrument_id = InstrumentId::new(reference.instrument_id.as_str()).map_err(invalid)?;
        let assembly = SnapshotAssembly {
            as_of: known_at,
            known_at,
            generation: Version::new(1).map_err(invalid)?,
            configuration: configuration.clone(),
            // The snapshot narrows the selection to one instrument and a request quote,
            // so it does not claim the activation's selection (D61).
            configuration_capture: None,
            instruments: vec![InstrumentPin {
                instrument_id: instrument_id.clone(),
                version: reference.version,
            }],
            // The mark is stamped at the cutoff, so it is never older than one millisecond.
            quotes: vec![QuoteRequirement {
                key: key.clone(),
                fields: vec![QuoteField::MidPrice],
                max_age_ms: 1,
            }],
            fixings: self.fixings.clone(),
            required_curves: roots
                .iter()
                .map(|id| CurveId::new(id.as_str()))
                .collect::<Result<_, _>>()
                .map_err(invalid)?,
            curves: vec![],
            curve_recipes: curves.records().to_vec(),
            calibration: None,
            max_source_skew_ms: self.max_source_skew_ms,
        };
        let events: Vec<_> = mark
            .map(|mid| request_mark(key, mid, known_at))
            .into_iter()
            .collect();
        let snapshot = self.assembler.resolve(&assembly, &events).await?;
        let valuation = ValuationContext::new(ValuationContextSpec {
            snapshot_id: snapshot.id().clone(),
            generation: snapshot.content().generation,
            subject: ValuationSubject::InstrumentVersion {
                instrument_id,
                version: reference.version,
            },
            valuation_time: known_at,
            known_at,
            settlement_date,
            configuration,
            model_id: ModelId::new(model).map_err(invalid)?,
            build_id: self.build_id.clone(),
            reporting_currency: bond.currency,
            fixing_known_at: known_at,
            scenario: ValuationScenario::Base,
            numerical_settings: self.numerical_settings.clone(),
            measures,
        })?;
        let pins = curves
            .pins()
            .map(|(id, version)| (id.to_owned(), version))
            .collect();
        Ok((snapshot, valuation, pins))
    }
}

/// The request's mark as one image observed at the cutoff. It is classed live: the
/// runtime values only valid inputs, and a manual observation classes as indicative.
/// Its source and session name it a request mark.
fn request_mark(key: QuoteKey, mid: Decimal, known_at: UtcTimestamp) -> QuoteEvent {
    let at = known_at.datetime().timestamp_millis();
    QuoteEvent {
        schema_version: 1,
        key,
        session: REQUEST_SOURCE.into(),
        sequence: 1,
        source_time_ms: at,
        receive_time_ms: at,
        quality: QuoteQuality::Live,
        entitled: true,
        entitlement_tags: vec![],
        evidence_reference: None,
        event: QuoteEventKind::Image {
            fields: vec![QuoteChange {
                field: QuoteField::MidPrice,
                update: FieldUpdate::Set(QuoteValue {
                    value: mid,
                    unit: QuoteUnit::PricePer100,
                }),
            }],
        },
    }
}
