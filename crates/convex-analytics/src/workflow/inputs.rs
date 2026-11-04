//! Actual supplied immutable resources and pre-valuation eligibility.
use super::*;
use convex_core::{
    contracts::UtcTimestamp,
    fixings::{FixingContext, FixingSeries, MAX_FIXING_REQUIREMENTS},
};
use convex_curves::contracts::CurveSet;
use std::collections::{BTreeMap, BTreeSet};

/// Explicit administrator publication schedule. Construction admits no duplicate
/// index/date entry, even if its timestamps match. No calendar/clock default.
#[derive(Debug, Clone)]
pub struct PublicationSchedule(BTreeMap<(IndexId, Date), UtcTimestamp>);
impl PublicationSchedule {
    /// Validate and freeze up to65,536 actual administrator decisions.
    pub fn new(entries: Vec<FixingRequirement>) -> WorkflowResult<Self> {
        if entries.len() > 65_536 {
            return Err(invalid("publications", "at most65536 entries"));
        }
        let mut map = BTreeMap::new();
        for e in entries {
            if !(1..=9999).contains(&e.observation_date.year()) {
                return Err(invalid("publication date", "outside0001..9999"));
            }
            if map
                .insert((e.index_id, e.observation_date), e.expected_publication)
                .is_some()
            {
                return Err(invalid("publications", "duplicate index/date"));
            }
        }
        Ok(Self(map))
    }
    /// Exact supplied decision, without a lookup outside this immutable bundle.
    pub fn get(&self, index: &IndexId, date: Date) -> WorkflowResult<UtcTimestamp> {
        self.0.get(&(index.clone(), date)).copied().ok_or_else(|| {
            WorkflowError::MissingInput(vec![MissingInput::Publication(index.clone(), date)])
        })
    }
    /// Callback for existing pure native APIs; wrapper routes retain typed
    /// missing-publication outcomes through `with_publications`.
    pub fn callback(
        &self,
    ) -> impl FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError> + '_ {
        |i, d| {
            self.get(i, d).map_err(|e| CashflowError::InvalidTerms {
                field: "publication",
                reason: e.to_string(),
            })
        }
    }
    /// Check a caller/native manifest against every exact declared timestamp.
    pub fn require(&self, requirements: &[FixingRequirement]) -> WorkflowResult<()> {
        for r in requirements {
            if self.get(&r.index_id, r.observation_date)? != r.expected_publication {
                return Err(invalid(
                    "publication",
                    "manifest differs from supplied administrator schedule",
                ));
            }
        }
        Ok(())
    }
}
pub(super) fn with_publications<T, E: Into<WorkflowError>>(
    schedule: &PublicationSchedule,
    f: impl FnOnce(
        &mut dyn FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
    ) -> Result<T, E>,
) -> WorkflowResult<T> {
    let mut missing = Vec::new();
    let mut callback = |i: &IndexId, d: Date| match schedule.get(i, d) {
        Ok(v) => Ok(v),
        Err(_) => {
            missing.push(MissingInput::Publication(i.clone(), d));
            Err(CashflowError::InvalidTerms {
                field: "publication",
                reason: "actual administrator schedule entry missing".into(),
            })
        }
    };
    let result = f(&mut callback);
    if !missing.is_empty() {
        return Err(WorkflowError::MissingInput(missing));
    }
    result.map_err(Into::into)
}
/// All market values are resolved and immutable; no repository-handle inputs.
#[derive(Clone, Copy)]
pub struct SuppliedMarket<'a> {
    /// Exact checked curve values and native recipe/query diagnostics.
    pub curves: &'a CurveSet,
    /// Actual eligible history, including original revisions/publication times.
    pub fixings: &'a FixingSeries,
    /// Explicit administrator decisions for requested observation dates.
    pub publications: &'a PublicationSchedule,
}
/// Eligibility before forecasts. Unknown and projected inputs remain separate.
#[derive(Debug, Clone)]
pub struct FixingRequirements {
    /// Actual original requirements.
    pub required: Vec<FixingRequirement>,
    /// Absent due history; cannot be replaced by projection.
    pub missing: Vec<FixingRequirement>,
    /// Eligible future/unpublished forecast requests, with no fabricated rates.
    pub projected: Vec<FixingRequirement>,
}
/// Inspect all native requirements without creating synthetic forecast values.
pub fn inspect_fixing_requirements(
    requirements: &[FixingRequirement],
    history: &FixingSeries,
    context: FixingContext,
) -> WorkflowResult<FixingRequirements> {
    if requirements.len() > 65_536 {
        return Err(invalid("requirements", "at most65536 unique observations"));
    }
    let mut keys = BTreeSet::new();
    let mut missing = Vec::new();
    let mut projected = Vec::new();
    for r in requirements {
        if !keys.insert((r.index_id.clone(), r.observation_date)) {
            return Err(invalid("requirements", "duplicate observation"));
        }
    }
    for batch in requirements.chunks(MAX_FIXING_REQUIREMENTS) {
        let resolved = history.resolve(batch, context)?;
        missing.extend_from_slice(resolved.missing());
        projected.extend(resolved.projected_requirements().cloned());
    }
    Ok(FixingRequirements {
        required: requirements.to_vec(),
        missing,
        projected,
    })
}
pub(super) fn resolve_requirements(
    requirements: &[FixingRequirement],
    history: &FixingSeries,
    context: FixingContext,
) -> WorkflowResult<FixingRequirements> {
    let r = inspect_fixing_requirements(requirements, history, context)?;
    if !r.missing.is_empty() {
        return Err(WorkflowError::MissingInput(
            r.missing.into_iter().map(MissingInput::Fixing).collect(),
        ));
    }
    Ok(r)
}
/// Small closed construction entry point, preserving all original native terms.
/// Calendar identities are supplied by each native binding; no string selects a
/// provider or an unversioned convention preset.
pub enum ConstructionInput<'a> {
    /// Fixed/floating/stepped/optional full bond contract.
    Bond {
        /// Complete original terms.
        terms: &'a convex_bonds::contracts::BondTerms,
        /// Actual independent calendars.
        calendars: &'a convex_bonds::contracts::BondCalendars<'a>,
    },
    /// Full IRS/OIS economics.
    Swap {
        /// Complete original terms.
        terms: &'a convex_rates::SwapTerms,
        /// Exact native calendar bindings.
        calendars: &'a convex_rates::SwapCalendars<'a>,
    },
    /// Full single-name CDS economics.
    Cds {
        /// Complete original terms.
        terms: &'a convex_credit::CdsTerms,
        /// Exact premium/settlement calendars.
        calendars: &'a convex_credit::CdsCalendars<'a>,
    },
    /// Actual dated disjoint futures reference; baskets/history/pricing are separate.
    Future(&'a convex_core::futures::FuturesContract),
}
/// Actual checked native instrument, retaining its original economics.
#[derive(Debug, Clone)]
pub enum ConstructedInstrument {
    /// Bond schedules, profiles and principal validated by the native builder.
    Bond(Box<convex_bonds::contracts::BondContract>),
    /// Swap independent leg/reset schedules validated by the native builder.
    Swap(Box<convex_rates::SwapContract>),
    /// CDS premium/protection/document economics validated by the native builder.
    Cds(Box<convex_credit::CdsContract>),
    /// Complete validated exchange reference, without execution authority.
    Future(Box<convex_core::futures::FuturesContract>),
}
/// Construct using actual calendars and the original complete native validation.
pub fn construct(input: ConstructionInput<'_>) -> WorkflowResult<ConstructedInstrument> {
    Ok(match input {
        ConstructionInput::Bond { terms, calendars } => {
            ConstructedInstrument::Bond(Box::new(terms.clone().build_with_calendars(calendars)?))
        }
        ConstructionInput::Swap { terms, calendars } => {
            ConstructedInstrument::Swap(Box::new(terms.clone().build(calendars)?))
        }
        ConstructionInput::Cds { terms, calendars } => {
            ConstructedInstrument::Cds(Box::new(terms.clone().build(calendars)?))
        }
        ConstructionInput::Future(terms) => {
            terms
                .validate()
                .map_err(|e| invalid("futures terms", e.to_string()))?;
            ConstructedInstrument::Future(Box::new(terms.clone()))
        }
    })
}
