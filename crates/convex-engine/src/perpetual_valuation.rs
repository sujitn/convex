//! Perpetuals on their workouts: a fixed-coupon callable perpetual quoted by price or by
//! its yield to worst is valued on each call date up to a horizon and to perpetuity, as
//! perpetuals are quoted, with no option model and no curve. From a clean price the model
//! solves the yield to worst; from a yield to worst it prices to worst. The yields are
//! dated yields under the bond's 30/360 day count, compounded at its coupon frequency.
//!
//! The horizon is the model's: the 128th call after settlement, or the last call whose
//! cash the pinned calendar covers within the shared schedule bounds. Every formula is the
//! pure crates' (`convex_analytics::workflow`): the engine builds the contract, its
//! context and its horizon only.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Mutex;

use convex_analytics::workflow::{
    quote_perpetual_price_to_worst, quote_perpetual_worst, BondInverseSettings, BondPriceTarget,
    DatedYieldConvention, PerpetualInputs, PerpetualPriceToWorst, PerpetualWorkout,
    PerpetualYieldToWorst, WorkoutContext, WorkoutSolveSettings,
};
use convex_bonds::contracts::{
    BondCalendars, BondContractError, BondRedemption, ExerciseAccrued, PerpetualCall,
    PerpetualContract, PerpetualTerms,
};
use convex_core::{
    calendars::Calendar,
    cashflows::{DateOffset, MAX_CONTRACT_DAYS},
    contracts::{ErrorCode, Version},
    ids::InstrumentId,
    Date,
};
use rust_decimal::Decimal;

use crate::level_pricing::LevelError;
use crate::optional_valuation::{failure, to_f64, Failure, CALENDAR_MARGIN_DAYS};
use crate::ports::{
    level_pricing::{PricingMethod, MAX_WORKOUT_CALLS},
    output::BondQuoteOutput,
    reference_data::BondReferenceData,
    reference_master::{CalendarDefinition, ResolvedBondReference},
    reference_repository::{
        validate_bond_reference, ReferenceChange, ReferenceCommand, ReferenceError, ReferenceRecord,
    },
};
use crate::pricing_router::PricingRouter;
use crate::workout_valuation::{
    calculate_stored, invalid, payment_calendar, street_convention, unnotified_context,
    workflow_failure, ValuedToWorst, WorkoutQuote,
};

/// Quantitative model for perpetuals: dated yields to each call within the horizon, and
/// the perpetuity's closed-form tail.
pub const PERPETUAL_SNAPSHOT_MODEL: &str = "convex-perpetual-workout-v1";
/// Measures a perpetual publishes.
pub const PERPETUAL_MEASURES: [&str; 4] = [
    "clean_price_mid",
    "dirty_price_mid",
    "accrued_interest",
    "ytw_mid",
];

/// Why a bond is not valued as a perpetual.
const NOT_A_PERPETUAL: &str = "only a perpetual with its perpetual terms is valued as one";

/// Why the library does not admit a stored perpetual its structural checks admitted.
pub const PERPETUAL_NOT_ADMITTED: &str = "the library does not build the perpetual's contract, such as a grid whose regular periods do not each count 1/f year under its day count";

// The callable workout model's settings, but for its stop. The solver stops when the price
// residual per 100 is below `parameter_tolerance`: 1e-13 is at the rounding of a sum over
// a hundred calls' cash, where a solve can stall, so the perpetual stops at 1e-12, as the
// library's perpetual tests do.
const WORST_SOLVE: WorkoutSolveSettings = WorkoutSolveSettings {
    inverse: BondInverseSettings {
        parameter_tolerance: 1e-12,
        ..crate::workout_valuation::WORST_SOLVE.inverse
    },
    ..crate::workout_valuation::WORST_SOLVE
};

/// A perpetual valued on its workouts.
#[derive(Debug, Clone)]
pub struct PerpetualValuation {
    /// Clean price per 100 face: the quoted price, or the price to worst.
    pub clean_price: Decimal,
    /// Dirty price per 100 face on the worst path.
    pub dirty_price: f64,
    /// Accrued per 100 face at settlement.
    pub accrued: f64,
    /// The lowest yield among the calls and the perpetuity, as a decimal.
    pub yield_to_worst: f64,
    /// The worst path: a call, or the perpetuity.
    pub worst: PerpetualWorkout,
}

impl PerpetualValuation {
    /// What every valuation to worst gives.
    pub(crate) fn to_worst(&self) -> ValuedToWorst {
        ValuedToWorst {
            clean_price: self.clean_price,
            dirty_price: self.dirty_price,
            accrued: self.accrued,
            yield_to_worst: self.yield_to_worst,
        }
    }
}

/// One path a perpetual is quoted to, with its yield at the calculated price.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerpetualPath {
    /// A call on its date, or the perpetuity.
    pub workout: PerpetualWorkout,
    /// The yield to it at the calculated clean price, as a decimal.
    pub decimal_yield: f64,
}

/// A perpetual calculated on its workouts: its valuation and each path's yield.
#[derive(Debug, Clone)]
pub struct PerpetualCalculation {
    /// The valuation, as Live and request-time quotes value the perpetual.
    pub valuation: PerpetualValuation,
    /// Each call whose notice can still be given, through the horizon, then the
    /// perpetuity.
    pub paths: Vec<PerpetualPath>,
    /// The path of lowest yield, on which the perpetual is valued.
    pub worst: PerpetualPath,
    /// The last call date quoted.
    pub horizon: Date,
    /// The price of every call, per 100 face.
    pub call_price: Decimal,
}

impl PerpetualCalculation {
    /// The calculation as a quote of `bond` at `settlement`: the clean price, accrued and
    /// yield to worst, as a request-time quote reads them from the perpetual model.
    pub fn bond_quote(&self, bond: &BondReferenceData, settlement: Date) -> BondQuoteOutput {
        self.valuation
            .to_worst()
            .bond_quote(bond, settlement, PERPETUAL_SNAPSHOT_MODEL)
    }
}

/// Calculate a stored perpetual on its workouts from `level`, quoted by `method` whatever
/// its rule quotes: its valuation, as Live and request-time quotes give it, and each path's
/// yield at its clean price. From a yield to worst, the yields are solved at the price to
/// worst, so the worst's yield is the level to the solver's tolerance.
pub fn calculate_perpetual(
    reference: &ResolvedBondReference,
    method: &PricingMethod,
    level: Decimal,
    settlement: Date,
) -> Result<PerpetualCalculation, LevelError> {
    calculate_stored(reference, method, level, settlement, calculate)
}

fn calculate(
    bond: &BondReferenceData,
    calendar: &CalendarDefinition,
    settlement: Date,
    quote: WorkoutQuote,
) -> Result<PerpetualCalculation, Failure> {
    let perpetual = StoredPerpetual::new(bond, calendar, settlement)?;
    let (valuation, yields) = match quote {
        WorkoutQuote::CleanPrice(clean) => {
            let yields = perpetual.yields_at_price(clean)?;
            (valuation_from_price(clean, &yields), yields)
        }
        WorkoutQuote::YieldToWorst(level) => {
            // Every path's yield is solved at the price to worst.
            let valuation = valuation_from_yield(&perpetual.prices_at_yield(level)?)?;
            let yields = perpetual.yields_at_price(valuation.clean_price)?;
            (valuation, yields)
        }
    };
    let paths: Vec<PerpetualPath> = yields
        .quotes
        .iter()
        .map(|path| PerpetualPath {
            workout: path.workout,
            decimal_yield: path.decimal_yield,
        })
        .collect();
    let worst = *paths
        .iter()
        .find(|path| path.workout == valuation.worst)
        .ok_or_else(|| invalid("the worst is not among the paths".into()))?;
    Ok(PerpetualCalculation {
        valuation,
        paths,
        worst,
        horizon: perpetual.horizon,
        call_price: perpetual.contract.perpetual().call.price_per_100,
    })
}

/// Value a stored perpetual on its workouts at `settlement`.
pub(crate) fn value_perpetual(
    bond: &BondReferenceData,
    calendar: &CalendarDefinition,
    settlement: Date,
    quote: WorkoutQuote,
) -> Result<PerpetualValuation, Failure> {
    let perpetual = StoredPerpetual::new(bond, calendar, settlement)?;
    match quote {
        WorkoutQuote::CleanPrice(clean) => Ok(valuation_from_price(
            clean,
            &perpetual.yields_at_price(clean)?,
        )),
        WorkoutQuote::YieldToWorst(level) => {
            valuation_from_yield(&perpetual.prices_at_yield(level)?)
        }
    }
}

/// Each perpetual's valuation, by what it depends on: the bond's version, which pins its
/// calendar's, the settlement and the quote. A tick on a benchmark security revalues a
/// perpetual whose rule names its set only for its G-spread; the valuation is read from
/// here. It holds at most 1,024 valuations and starts again when full.
#[derive(Debug, Default)]
pub struct PerpetualValuationCache {
    valued: Mutex<HashMap<ValuationKey, Result<PerpetualValuation, Failure>>>,
}

/// What a perpetual's valuation depends on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ValuationKey {
    instrument_id: InstrumentId,
    version: Version,
    settlement: Date,
    quote: WorkoutQuote,
}

const MAX_CACHED_VALUATIONS: usize = 1_024;

impl PerpetualValuationCache {
    /// [`value_perpetual`] of the stored bond `record` holds, on its pinned `calendar`,
    /// once for each version, settlement and quote.
    pub(crate) fn value(
        &self,
        record: &ReferenceRecord,
        calendar: &CalendarDefinition,
        settlement: Date,
        quote: WorkoutQuote,
    ) -> Result<PerpetualValuation, Failure> {
        let bond = record
            .bond
            .as_ref()
            .ok_or_else(|| invalid("the stored reference is deleted".into()))?;
        let key = ValuationKey {
            instrument_id: record.instrument_id.clone(),
            version: record.version,
            settlement,
            quote,
        };
        // A poisoned lock only loses the shared valuations; each is valued again.
        if let Some(valued) = self
            .valued
            .lock()
            .ok()
            .and_then(|valued| valued.get(&key).cloned())
        {
            return valued;
        }
        let valued = value_perpetual(bond, calendar, settlement, quote);
        if let Ok(mut cached) = self.valued.lock() {
            if cached.len() >= MAX_CACHED_VALUATIONS {
                cached.clear();
            }
            cached.insert(key, valued.clone());
        }
        valued
    }
}

/// The valuation at a quoted clean price, from its paths' yields.
fn valuation_from_price(clean: Decimal, yields: &PerpetualYieldToWorst) -> PerpetualValuation {
    let path = &yields.quotes[yields.selected_index];
    PerpetualValuation {
        clean_price: clean,
        dirty_price: path.dirty_per100,
        accrued: path.accrued_per100,
        yield_to_worst: path.decimal_yield,
        worst: path.workout,
    }
}

/// The valuation at a quoted yield to worst, from its paths' prices.
fn valuation_from_yield(prices: &PerpetualPriceToWorst) -> Result<PerpetualValuation, Failure> {
    let path = &prices.quotes[prices.selected_index];
    Ok(PerpetualValuation {
        clean_price: Decimal::from_f64_retain(path.clean_per100)
            .ok_or_else(|| invalid("the price to worst is not finite".into()))?,
        dirty_price: path.dirty_per100,
        accrued: path.accrued_per100,
        yield_to_worst: prices.decimal_yield,
        worst: path.workout,
    })
}

/// Admit each perpetual `command` creates or updates: its stored terms first, then the
/// library's own perpetual admission, which also holds each regular period to 1/f year. A
/// dated bond is admitted by its repository alone. The library's reason is logged.
pub fn admit_perpetuals(command: &ReferenceCommand) -> Result<(), ReferenceError> {
    for change in &command.changes {
        let (ReferenceChange::Create { bond } | ReferenceChange::Update { bond, .. }) = change
        else {
            continue;
        };
        if !bond.is_perpetual() {
            continue;
        }
        validate_bond_reference(bond)?;
        if let Err(failure) = perpetual_contract(bond) {
            tracing::info!(
                instrument_id = bond.instrument_id.as_str(),
                reason = failure.message.as_str(),
                "perpetual refused by the library"
            );
            return Err(ReferenceError::InvalidInput(PERPETUAL_NOT_ADMITTED));
        }
    }
    Ok(())
}

/// A stored perpetual's library contract: the router's fixed-coupon schedule to its first
/// call, then its calls at their interval and price on calendar days' notice. Each call
/// pays its price and accrued coupon on its date, adjusted as the bond's payments are.
pub(crate) fn perpetual_contract(bond: &BondReferenceData) -> Result<PerpetualContract, Failure> {
    let perpetual = bond
        .contractual_terms
        .as_ref()
        .and_then(|terms| terms.perpetual.as_ref())
        .filter(|_| bond.is_perpetual())
        .ok_or_else(|| failure(ErrorCode::UnsupportedConvention, NOT_A_PERPETUAL))?;
    let notice = i32::try_from(perpetual.notice_days)
        .map_err(|_| invalid("the notice period overflows".into()))?;
    let mut terms = PricingRouter { bump_bps: 1.0 }
        .fixed_rate_bond_ending(bond, perpetual.first_call_date.date())
        .map_err(|e| invalid(e.to_string()))?
        .contract_terms()
        .map_err(library)?;
    terms.schema_version = 2;
    terms.redemption = BondRedemption::Perpetual(PerpetualTerms {
        call: PerpetualCall {
            interval_months: perpetual.call_interval_months,
            price_per_100: perpetual.call_price.decimal(),
            notice_offset: DateOffset::CalendarDays(-notice),
            settlement_lag: DateOffset::CalendarDays(0),
            settlement_adjustment: terms.dates.payment_adjustment,
            accrued: ExerciseAccrued::PayAccrued,
        },
        reset: None,
    });
    terms.build_perpetual().map_err(library)
}

/// A stored perpetual bought at one settlement, as the perpetual workflow takes it.
struct StoredPerpetual {
    contract: PerpetualContract,
    /// The router builds a stored bond on one calendar for payment, reset, observation and
    /// exercise.
    calendar: Box<dyn Calendar>,
    context: WorkoutContext,
    convention: DatedYieldConvention,
    horizon: Date,
}

impl StoredPerpetual {
    fn new(
        bond: &BondReferenceData,
        calendar: &CalendarDefinition,
        settlement: Date,
    ) -> Result<Self, Failure> {
        let contract = perpetual_contract(bond)?;
        let source = contract.terms();
        let coverage = |reason: &str| failure(ErrorCode::UnsupportedConvention, reason);
        if source.schedule.start_date.add_days(-CALENDAR_MARGIN_DAYS)
            < calendar.coverage_from.date()
        {
            return Err(coverage(
                "the pinned calendar's coverage does not start before the perpetual's accrual",
            ));
        }
        let last_end = calendar
            .coverage_until
            .date()
            .add_days(-CALENDAR_MARGIN_DAYS)
            .min(source.schedule.start_date.add_days(MAX_CONTRACT_DAYS));
        let horizon = last_quoted_call(&contract, settlement, last_end)?.ok_or_else(|| {
            coverage("the pinned calendar's coverage ends before the perpetual's next call")
        })?;
        let payment = payment_calendar(source)?;
        let context = unnotified_context(source, payment.as_ref(), settlement)?;
        let convention = street_convention(source);
        Ok(Self {
            contract,
            calendar: payment,
            context,
            convention,
            horizon,
        })
    }

    /// `quote` with the perpetual's inputs, every role on its one calendar.
    fn with_inputs<T>(&self, quote: impl FnOnce(&PerpetualInputs<'_>) -> T) -> T {
        quote(&PerpetualInputs {
            contract: &self.contract,
            context: &self.context,
            calendar_ids: &self.contract.terms().calendars,
            calendars: &one_calendar(self.calendar.as_ref()),
            horizon: self.horizon,
            reset_market: None,
        })
    }

    /// Every path's yield at `clean`, and the worst.
    fn yields_at_price(&self, clean: Decimal) -> Result<PerpetualYieldToWorst, Failure> {
        let target = BondPriceTarget::Clean(to_f64(clean, "clean price")?);
        self.with_inputs(|inputs| {
            quote_perpetual_worst(inputs, self.convention, target, WORST_SOLVE)
                .map_err(workflow_failure)
        })
    }

    /// Every path's price at `yield_to_worst`, and the lowest.
    fn prices_at_yield(&self, yield_to_worst: Decimal) -> Result<PerpetualPriceToWorst, Failure> {
        let level = to_f64(yield_to_worst, "yield")?;
        self.with_inputs(|inputs| {
            quote_perpetual_price_to_worst(inputs, self.convention, level).map_err(workflow_failure)
        })
    }
}

/// The cash a stored perpetual pays a holder between two settlement dates, per 100 face:
/// its coupons. No call is taken, so it pays no principal: the dated composition ends
/// after `to`.
pub(crate) fn perpetual_entitled_cash(
    bond: &BondReferenceData,
    from: Date,
    to: Date,
) -> Result<Decimal, Failure> {
    let contract = perpetual_contract(bond)?;
    let calendar = payment_calendar(contract.terms())?;
    let end = contract.coupon_date_after(to).map_err(library)?;
    contract
        .dated_through(
            end,
            contract.first_call(),
            &[],
            &one_calendar(calendar.as_ref()),
        )
        .and_then(|dated| dated.entitled_cash(from, to))
        .map_err(library)
}

/// A stored bond's calendars: its one calendar in every role.
fn one_calendar(calendar: &dyn Calendar) -> BondCalendars<'_> {
    BondCalendars {
        payment: calendar,
        reset: calendar,
        observation: calendar,
        exercise: calendar,
    }
}

/// The last call quoted from `settlement`: the [`MAX_WORKOUT_CALLS`]th call after it, or
/// the last whose dated composition ends by `last_end`. None when no call does.
fn last_quoted_call(
    contract: &PerpetualContract,
    settlement: Date,
    last_end: Date,
) -> Result<Option<Date>, Failure> {
    let mut last = None;
    for call in contract
        .call_dates(settlement, last_end)
        .map_err(library)?
        .into_iter()
        .take(MAX_WORKOUT_CALLS)
    {
        if contract.coupon_date_after(call).map_err(library)? > last_end {
            break;
        }
        last = Some(call);
    }
    Ok(last)
}

/// A refusal of the library's contract, as invalid input.
fn library(error: BondContractError) -> Failure {
    invalid(error.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ports::reference_master::MasterDefinition;
    use convex_core::contracts::BusinessDate;
    use serde_json::Value;
    use std::str::FromStr;

    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }
    fn decimal(value: &Value) -> Decimal {
        Decimal::from_str(value.as_str().unwrap()).unwrap()
    }
    fn float(value: &Value) -> f64 {
        value.as_str().unwrap().parse().unwrap()
    }
    fn reference() -> Value {
        serde_json::from_str(include_str!(
            "../../convex-analytics/tests/fixtures/perpetual-quotes.json"
        ))
        .unwrap()
    }
    /// The fixture's dated bond.
    fn dated() -> BondReferenceData {
        serde_json::from_str(include_str!(
            "../../convex-ports/tests/fixtures/reference_bond.json"
        ))
        .unwrap()
    }
    /// The reference's fixed perpetual, stored.
    fn perpetual() -> BondReferenceData {
        crate::products::perpetual(dated())
    }
    /// The calendar the stored perpetual pins, covering 1900 to 2100.
    fn calendar(bond: &BondReferenceData) -> CalendarDefinition {
        let resources = crate::reference_terms::resources(bond);
        let MasterDefinition::Calendar(calendar) = &resources[1].definition else {
            panic!("the calendar resource")
        };
        (**calendar).clone()
    }
    fn workout(text: &str) -> PerpetualWorkout {
        match text {
            "perpetuity" => PerpetualWorkout::Perpetuity,
            call => PerpetualWorkout::Call { date: date(call) },
        }
    }

    #[test]
    fn every_quoted_path_and_the_worst_are_the_reference() {
        let bond = perpetual();
        let calendar = calendar(&bond);
        let cases = reference()["bonds"][0]["cases"].clone();
        for case in cases.as_array().unwrap() {
            let settlement = date(case["settlement"].as_str().unwrap());
            for quote in case["yield_to_worst"].as_array().unwrap() {
                let clean = decimal(&quote["clean_price"]);
                let calculated = calculate(
                    &bond,
                    &calendar,
                    settlement,
                    WorkoutQuote::CleanPrice(clean),
                )
                .unwrap();
                // The model quotes more calls than the reference's horizon; each the
                // reference quotes, and the perpetuity, has its yield.
                for path in quote["yields"].as_array().unwrap() {
                    let workout = workout(path["workout"].as_str().unwrap());
                    let ours = calculated
                        .paths
                        .iter()
                        .find(|ours| ours.workout == workout)
                        .unwrap();
                    let expected = float(&path["yield"]);
                    assert!(
                        (ours.decimal_yield - expected).abs() < 1e-10,
                        "{case}: {workout:?} {} against {expected}",
                        ours.decimal_yield
                    );
                }
                // Through the reference's horizon, the calls are the reference's.
                let horizon = date(case["horizon"].as_str().unwrap());
                let calls: Vec<PerpetualWorkout> = calculated
                    .paths
                    .iter()
                    .map(|path| path.workout)
                    .filter(
                        |path| matches!(path, PerpetualWorkout::Call { date } if *date <= horizon),
                    )
                    .collect();
                let expected: Vec<PerpetualWorkout> = case["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|call| workout(call.as_str().unwrap()))
                    .collect();
                assert_eq!(calls, expected);
                let worst = workout(quote["worst"].as_str().unwrap());
                assert_eq!(calculated.worst.workout, worst, "{clean}");
                assert_eq!(calculated.valuation.worst, worst);
                assert_eq!(
                    calculated.valuation.yield_to_worst,
                    calculated.worst.decimal_yield
                );
                let accrued = float(&case["accrued_per100"]);
                assert!((calculated.valuation.accrued - accrued).abs() < 1e-12);
            }
            for quote in case["price_to_worst"].as_array().unwrap() {
                let level = decimal(&quote["yield"]);
                let valued = value_perpetual(
                    &bond,
                    &calendar,
                    settlement,
                    WorkoutQuote::YieldToWorst(level),
                )
                .unwrap();
                let worst = workout(quote["worst"].as_str().unwrap());
                assert_eq!(valued.worst, worst, "{level}");
                let expected = quote["prices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|price| workout(price["workout"].as_str().unwrap()) == worst)
                    .unwrap();
                assert!(
                    (valued.clean_price - decimal(&expected["clean"])).abs() < Decimal::new(1, 9),
                    "{level}: {} against {}",
                    valued.clean_price,
                    expected["clean"]
                );
                assert_eq!(valued.yield_to_worst, float(&quote["yield"]));
            }
        }
    }

    #[test]
    fn calls_are_quoted_to_the_128th_or_as_far_as_the_calendar_covers() {
        let bond = perpetual();
        let settlement = date("2026-03-10");
        let price = WorkoutQuote::CleanPrice(Decimal::from(100));
        let mut covered = calendar(&bond);
        // Semiannual calls from January 2030: the 128th is in July 2093, 63.5 years on.
        let calculated = calculate(&bond, &covered, settlement, price).unwrap();
        assert_eq!(calculated.horizon, date("2093-07-15"));
        assert_eq!(calculated.paths.len(), 129);
        assert_eq!(
            calculated.paths.last().unwrap().workout,
            PerpetualWorkout::Perpetuity
        );
        // Covered to 2040, a call is quoted only when the coupon date after it is covered,
        // with a month's margin.
        covered.coverage_until = BusinessDate::parse("2040-12-31").unwrap();
        let calculated = calculate(&bond, &covered, settlement, price).unwrap();
        assert_eq!(calculated.horizon, date("2040-01-15"));
        assert_eq!(calculated.paths.len(), 22);
        covered.coverage_until = BusinessDate::parse("2030-07-31").unwrap();
        let refused = calculate(&bond, &covered, settlement, price).err().unwrap();
        assert_eq!(refused.code, ErrorCode::UnsupportedConvention);
        assert!(refused.message.contains("next call"), "{}", refused.message);
    }

    #[test]
    fn a_call_whose_notice_has_passed_is_not_quoted() {
        let bond = perpetual();
        let calendar = calendar(&bond);
        let first_call = |settlement: &str| {
            calculate(
                &bond,
                &calendar,
                date(settlement),
                WorkoutQuote::CleanPrice(Decimal::from(100)),
            )
            .unwrap()
            .paths[0]
                .workout
        };
        // The January 2030 call's 30 days' notice falls on 16 December 2029: a trade on
        // 30 November may still be called then, one on 26 December may not.
        assert_eq!(
            first_call("2029-12-04"),
            PerpetualWorkout::Call {
                date: date("2030-01-15")
            }
        );
        assert_eq!(
            first_call("2029-12-28"),
            PerpetualWorkout::Call {
                date: date("2030-07-15")
            }
        );
    }

    #[test]
    fn a_valuation_is_kept_for_its_version_settlement_and_quote() {
        let record = |version: u64, bond: BondReferenceData| ReferenceRecord {
            schema_version: convex_core::contracts::SchemaVersion::V2,
            instrument_id: bond.instrument_id.clone(),
            version: Version::new(version).unwrap(),
            recorded_at: convex_core::contracts::UtcTimestamp::parse("2025-06-01T00:00:00Z")
                .unwrap(),
            bond: Some(bond),
        };
        let stored = record(1, perpetual());
        let calendar = calendar(&perpetual());
        let cache = PerpetualValuationCache::default();
        let value = |record: &ReferenceRecord, settlement: &str, clean: Decimal| {
            cache
                .value(
                    record,
                    &calendar,
                    date(settlement),
                    WorkoutQuote::CleanPrice(clean),
                )
                .unwrap()
                .yield_to_worst
        };
        let premium = value(&stored, "2026-10-06", Decimal::new(10425, 2));
        assert_eq!(
            value(&stored, "2026-10-06", Decimal::new(10425, 2)),
            premium
        );
        // Another quote, settlement or version is valued afresh.
        assert_ne!(value(&stored, "2026-10-06", Decimal::new(925, 1)), premium);
        assert_ne!(
            value(&stored, "2027-10-06", Decimal::new(10425, 2)),
            premium
        );
        let mut revised = perpetual();
        revised.coupon_rate = Some(Decimal::new(7, 2));
        assert_ne!(
            value(&record(2, revised), "2026-10-06", Decimal::new(10425, 2)),
            premium
        );
    }

    #[test]
    fn a_perpetual_builds_no_dated_bond_and_a_dated_bond_no_perpetual() {
        let bond = perpetual();
        assert!(PricingRouter::new().to_fixed_rate_bond(&bond).is_err());
        assert_eq!(
            perpetual_contract(&dated()).err().unwrap().message,
            NOT_A_PERPETUAL
        );
    }

    #[test]
    fn a_holder_is_entitled_to_the_coupons_between_two_settlements_and_no_principal() {
        let bond = perpetual();
        let cash = |from: &str, to: &str| perpetual_entitled_cash(&bond, date(from), date(to));
        // 6.5% semiannual: 3.25 per 100 on each coupon date, the first call's included.
        assert_eq!(cash("2026-03-10", "2026-03-20").unwrap(), Decimal::ZERO);
        assert_eq!(
            cash("2029-12-01", "2030-02-01").unwrap(),
            Decimal::new(325, 2)
        );
        assert_eq!(
            cash("2030-01-01", "2030-08-01").unwrap(),
            Decimal::new(650, 2)
        );
        assert_eq!(
            cash("2031-06-01", "2032-06-01").unwrap(),
            Decimal::new(650, 2)
        );
        // Sold for settlement on a coupon date, the holder is still paid that coupon, and
        // no principal is due.
        assert_eq!(
            cash("2030-01-01", "2030-07-15").unwrap(),
            Decimal::new(650, 2)
        );
    }

    #[test]
    fn the_library_admits_only_a_perpetual_whose_periods_each_count_a_half_year() {
        let creating = |bond: BondReferenceData| ReferenceCommand {
            idempotency_key: convex_core::contracts::RequestId::new("admission").unwrap(),
            changes: vec![ReferenceChange::Create {
                bond: Box::new(bond),
            }],
            master_changes: vec![],
        };
        assert!(admit_perpetuals(&creating(perpetual())).is_ok());
        // The stored-terms checks admit a month-end grid reaching February; the library,
        // counting its last day short, does not.
        let month_end = crate::products::month_end_perpetual(dated());
        validate_bond_reference(&month_end).unwrap();
        assert!(matches!(
            admit_perpetuals(&creating(month_end)),
            Err(ReferenceError::InvalidInput(PERPETUAL_NOT_ADMITTED))
        ));
        // A dated bond is left to its repository.
        assert!(admit_perpetuals(&creating(dated())).is_ok());
    }
}
