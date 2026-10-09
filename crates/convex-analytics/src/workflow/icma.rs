//! Checked contextual ICMA quotations over original native fixed/workout cash.
use super::*;
use convex_bonds::{contracts::BondContract, types::CalendarId};
use convex_core::{
    calendars::Calendar,
    cashflows::{
        icma_year_fraction, validate_icma_reference_periods, PaymentDateEntitlement,
        ReferencePeriod,
    },
    contracts::ModelMoney,
    types::{Compounding, Frequency},
};
use convex_rates::ReferencePin;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum examined reference entries while preparing one original cash quote.
pub const MAX_ICMA_REFERENCE_EVALUATIONS: u64 = 16_777_216;

/// Actual immutable nominal reference context, independent of cash payment dates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaReferenceContext {
    /// Source's exact immutable economic reference revision.
    pub revision: ReferencePin,
    /// Actual security reference to which the nominal mesh belongs.
    pub security: ReferencePin,
    /// Explicit normal determination frequency, never guessed from day lengths.
    pub frequency: Frequency,
    /// Complete original anchored nominal periods, including any explicit tail.
    pub periods: Vec<ReferencePeriod>,
}
/// Exact quotation policy, independent of contractual coupon accrual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaYieldConvention {
    /// Version1 of this supplied contextual quotation composition.
    pub schema_version: u32,
    /// Exact independently supplied reference context revision.
    pub reference_context: ReferencePin,
    /// Exact original supplied policy revision; no market policy is inferred.
    pub quotation_policy: ReferencePin,
    /// Explicit normal determination frequency, distinct from compounding.
    pub frequency: Frequency,
    /// Original annualized yield compounding applied to contextual time.
    pub compounding: Compounding,
    /// Positive ceiling on examined reference entries while preparing cash times.
    pub max_reference_evaluations: u64,
}
/// Actual independent source calendar revisions, bound by the embedding caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaCalendarPins {
    /// Original payment/accrual resource.
    pub payment: ReferencePin,
    /// Original reset resource, retained even for fixed cash.
    pub reset: ReferencePin,
    /// Original observation resource, retained even for fixed cash.
    pub observation: ReferencePin,
    /// Original legal exercise/notice resource.
    pub exercise: ReferencePin,
}
/// Exact quotation provenance; neither provider nor calendar lookup participates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaQuotationSources {
    /// Original full security reference revision.
    pub security: ReferencePin,
    /// Independent original observed price/yield revision.
    pub quote_revision: ReferencePin,
    /// Original resolved quotation policy revision.
    pub quotation_policy: ReferencePin,
    /// Full actual calendar versions used at native construction.
    pub calendars: IcmaCalendarPins,
}
/// Disjoint original plain cash or legal conditional exercise cash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IcmaCashSelection {
    /// Original plain fixed/stepped/amortizing cash under original model guards.
    Plain {
        /// Actual purchase settlement boundary.
        settlement: Date,
        /// Explicit ownership of same-day scheduled payments.
        on_payment: PaymentDateEntitlement,
    },
    /// Complete legally permitted conditional fixed workout, without option value.
    Workout {
        /// Full actual purchase/information/notice state.
        context: WorkoutContext,
        /// Exact maturity or original legal provision/date.
        selection: WorkoutSelection,
    },
}
/// Complete native supplied inputs. Calendar data must match these original pins;
/// the generic shared API performs exact resource revision resolution.
pub struct IcmaQuotationInputs<'a> {
    /// Original checked complete fixed bond economics and schedules.
    pub contract: &'a BondContract,
    /// Independently resolved full source identities/revisions.
    pub sources: &'a IcmaQuotationSources,
    /// Full original explicit nominal determination mesh.
    pub reference_context: &'a IcmaReferenceContext,
    /// Original exact quote convention and reference-work policy.
    pub convention: &'a IcmaYieldConvention,
    /// Exact original-face signed quantity/currency; outstanding face cannot replace it.
    pub position: &'a crate::quote_risk::RiskPosition,
    /// Full original exercise calendar bound to the contract.
    pub exercise_calendar: (&'a CalendarId, &'a dyn Calendar),
    /// Full original purchase calendar bound to the contract.
    pub purchase_calendar: (&'a CalendarId, &'a dyn Calendar),
}
/// Private prepared state prevents fabricated cash/times/accrued/source inputs.
/// All contextual times are checked once, before any price or inverse endpoint.
#[derive(Debug, Clone)]
pub struct PreparedIcmaQuotation {
    cash: PreparedBondCash,
    sources: IcmaQuotationSources,
    reference_context: IcmaReferenceContext,
    convention: IcmaYieldConvention,
    selection: IcmaCashSelection,
    source_cash: Option<convex_bonds::contracts::BondProjection>,
    exercise: Option<WorkoutCashDetails>,
    position: crate::quote_risk::RiskPosition,
    scale: f64,
    times: BTreeMap<Date, f64>,
    reserved_reference_evaluations: u64,
}
/// Full original cash quotation, source context and signed consideration.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaQuotation {
    /// Complete original economics, including accrual and all legal terms.
    pub terms: convex_bonds::contracts::BondTerms,
    /// Actual independent security/quote/policy/calendar source pins.
    pub sources: IcmaQuotationSources,
    /// Full original nominal reference mesh and its exact revision.
    pub reference_context: IcmaReferenceContext,
    /// Original quote compounding/frequency/work and revision policy.
    pub convention: IcmaYieldConvention,
    /// Original purchase/cash entitlement and conditional legal choice.
    pub selection: IcmaCashSelection,
    /// Original unexercised full projection retained for conditional workouts.
    pub source_cash: Option<convex_bonds::contracts::BondProjection>,
    /// Actual legal redemption dates/economics, separate from delayed claims.
    pub exercise: Option<WorkoutCashDetails>,
    /// Complete original-face quantity and currency.
    pub position: crate::quote_risk::RiskPosition,
    /// Original computed full price or accepted same-pricer inverse.
    pub quotation: WorkoutValue,
    /// Price uses computed dirty; inverse uses ORIGINAL observed dirty target.
    pub consideration: ModelMoney,
    /// Exact contextual times for every distinct actual used payment date.
    pub payment_times: BTreeMap<Date, f64>,
    /// Conservative original reference-entry reservation for one-time preparation.
    /// Includes validation/comparison/map visits, original coupon checks and two
    /// traversals per distinct payment. Primitive schedule bounds remain separate.
    pub reserved_reference_evaluations: u64,
}
impl PreparedIcmaQuotation {
    /// Build full original plain or conditional cash and admit every time/source.
    /// Missing, overlapping, conflicting or uncovered context never falls back.
    pub fn new(
        input: &IcmaQuotationInputs<'_>,
        selection: &IcmaCashSelection,
    ) -> WorkflowResult<Self> {
        let t = input.contract.terms();
        let c = input.convention;
        let r = input.reference_context;
        let s = input.sources;
        if c.schema_version != 1 || c.frequency.is_zero() || r.frequency.is_zero() {
            return Err(WorkflowError::Unsupported(
                "ICMA version1 and a normal coupon frequency required",
            ));
        }
        if c.reference_context != r.revision
            || c.quotation_policy != s.quotation_policy
            || r.security != s.security
            || c.frequency != r.frequency
            || r.frequency != t.schedule.frequency
        {
            return Err(invalid(
                "ICMA sources",
                "exact security/context/policy revisions and original normal frequency required",
            ));
        }
        for (pin, id) in [
            (&s.calendars.payment, &t.calendars.payment),
            (&s.calendars.reset, &t.calendars.reset),
            (&s.calendars.observation, &t.calendars.observation),
            (&s.calendars.exercise, &t.calendars.exercise),
        ] {
            if pin.id.as_str() != id.as_str() {
                return Err(invalid(
                    "ICMA calendars",
                    "original actual independent calendar identities required",
                ));
            }
        }
        if input.exercise_calendar.0 != &t.calendars.exercise
            || input.purchase_calendar.0 != &t.calendars.payment
        {
            return Err(invalid(
                "ICMA calendars",
                "exact original exercise and purchase calendars required",
            ));
        }
        validate_icma_reference_periods(&t.schedule, &r.periods)?;
        // Every original coupon's actual reference periods must survive, including
        // long stubs and periods before the purchase or hypothetical exercise.
        let supplied: BTreeMap<_, _> = r.periods.iter().map(|p| (p.start, p.end)).collect();
        let mut original_references = 0_u64;
        for period in input.contract.periods() {
            for reference in period.reference_periods() {
                original_references += 1;
                if supplied.get(&reference.start) != Some(&reference.end) {
                    return Err(invalid(
                        "ICMA references",
                        "full original coupon reference periods required",
                    ));
                }
            }
        }
        let scale = crate::cash_quotation::invoice_scale(t, input.position).map_err(cash_error)?;
        let (cash, source_cash, exercise) = match selection {
            IcmaCashSelection::Plain {
                settlement,
                on_payment,
            } => (
                PreparedBondCash::fixed(input.contract, *settlement, *on_payment)?,
                None,
                None,
            ),
            IcmaCashSelection::Workout { context, selection } => {
                let w = WorkoutInputs {
                    contract: input.contract,
                    context,
                    exercise_calendar: input.exercise_calendar,
                    purchase_calendar: input.purchase_calendar,
                };
                super::workout::purchase(&w)?;
                let p = super::workout::prepare(
                    input.contract,
                    context,
                    selection,
                    input.exercise_calendar.0,
                    input.exercise_calendar.1,
                )?;
                (p.cash, Some(p.source_cash), p.exercise)
            }
        };
        cash.fixed_yield()?;
        let dates: BTreeSet<_> = cash
            .cashflows()
            .cashflows()?
            .into_iter()
            .map(|c| c.date)
            .collect();
        let reserved_reference_evaluations = (2 * dates.len() as u64 + 3)
            .checked_mul(r.periods.len() as u64)
            .and_then(|v| v.checked_add(original_references))
            .ok_or(CashflowError::BoundExceeded("ICMA reference evaluations"))?;
        if c.max_reference_evaluations == 0
            || c.max_reference_evaluations > MAX_ICMA_REFERENCE_EVALUATIONS
            || reserved_reference_evaluations > c.max_reference_evaluations
        {
            return Err(CashflowError::BoundExceeded("ICMA reference evaluations").into());
        }
        let mut times = BTreeMap::new();
        for date in dates {
            // Even zero-time payments require actual explicit coverage.
            let (Some(first), Some(last)) = (r.periods.first(), r.periods.last()) else {
                return Err(invalid(
                    "ICMA references",
                    "explicit original reference coverage required",
                ));
            };
            if cash.settlement() < first.start || date > last.end || date < cash.settlement() {
                return Err(invalid(
                    "ICMA coverage",
                    "original settlement and every actual payment must be covered",
                ));
            }
            let decimal = icma_year_fraction(cash.settlement(), date, r.frequency, &r.periods)?;
            let time = decimal
                .to_f64()
                .filter(|v| v.is_finite() && (decimal.is_zero() || *v > 0.0))
                .ok_or_else(|| invalid("ICMA time", "finite original contextual time required"))?;
            times.insert(date, time);
        }
        // Cash-free states still cannot bypass source coverage admission.
        if !r
            .periods
            .iter()
            .any(|p| p.start <= cash.settlement() && cash.settlement() <= p.end)
        {
            return Err(invalid(
                "ICMA coverage",
                "original settlement must be covered",
            ));
        }
        Ok(Self {
            cash,
            sources: s.clone(),
            reference_context: r.clone(),
            convention: c.clone(),
            selection: selection.clone(),
            source_cash,
            exercise,
            position: input.position.clone(),
            scale,
            times,
            reserved_reference_evaluations,
        })
    }
    fn price(&self, decimal_rate: f64) -> WorkflowResult<BondValue> {
        if !decimal_rate.is_finite() || decimal_rate.abs() > 10.0 {
            return Err(invalid(
                "yield",
                "finite annual decimal rate within +/-10 required",
            ));
        }
        self.cash.price(
            |date| {
                let time = self
                    .times
                    .get(&date)
                    .ok_or_else(|| invalid("ICMA time", "original prepared payment required"))?;
                Ok(convex_curves::contracts::discount_from_rate(
                    decimal_rate,
                    *time,
                    self.convention.compounding,
                )?)
            },
            BondValueBasis::ContextualIcma {
                decimal_rate,
                convention: self.convention.clone(),
                reference_context: self.reference_context.clone(),
            },
        )
    }
    fn output(
        &self,
        quotation: WorkoutValue,
        original_dirty: f64,
    ) -> WorkflowResult<IcmaQuotation> {
        let consideration = crate::cash_quotation::invoice_consideration(
            self.cash.contract().terms(),
            original_dirty,
            self.scale,
        )
        .map_err(cash_error)?;
        Ok(IcmaQuotation {
            terms: self.cash.contract().terms().clone(),
            sources: self.sources.clone(),
            reference_context: self.reference_context.clone(),
            convention: self.convention.clone(),
            selection: self.selection.clone(),
            source_cash: self.source_cash.clone(),
            exercise: self.exercise.clone(),
            position: self.position.clone(),
            quotation,
            consideration,
            payment_times: self.times.clone(),
            reserved_reference_evaluations: self.reserved_reference_evaluations,
        })
    }
    /// Price original entitled fixed/workout cash at the supplied annual yield.
    pub fn quote_price(&self, decimal_rate: f64) -> WorkflowResult<IcmaQuotation> {
        let v = self.price(decimal_rate)?;
        let dirty = v.dirty_per100;
        self.output(WorkoutValue::Price(Box::new(v)), dirty)
    }
    /// Original closed-domain same-pricer inverse and final native residual.
    /// No expanded domain, inferred Street model or arbitrary mixed-sign root.
    pub fn quote_yield(
        &self,
        target: BondPriceTarget,
        settings: BondInverseSettings,
    ) -> WorkflowResult<IcmaQuotation> {
        let v = super::inverse::solve_bond(&self.cash, target, settings, 10.0, |p| self.price(p))?;
        let dirty = match target {
            BondPriceTarget::Dirty(d) => d,
            BondPriceTarget::Clean(c) => c + v.valuation.accrued_per100,
        };
        self.output(WorkoutValue::Yield(Box::new(v)), dirty)
    }
}
fn cash_error(error: crate::cash_quotation::CashQuotationError) -> WorkflowError {
    match error {
        crate::cash_quotation::CashQuotationError::Workflow(e) => e,
        e => invalid("ICMA invoice", e.to_string()),
    }
}
