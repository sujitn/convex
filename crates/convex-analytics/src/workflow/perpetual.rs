//! Callable perpetuals quoted to each call date within a horizon and to perpetuity, as
//! perpetuals are quoted, with no option model and no curve.
//!
//! A call's cash is the dated composition of its coupons and redemption, through the
//! same paths and notice rules as a dated bond's workouts. The perpetuity's is every
//! coupon to the tail start, then the coupon in force for ever, valued in closed form:
//! one period's discount `d` at the yield gives `C·d/(1−d)` at the tail start. Each
//! period after the tail start counts 1/f year and pays on its unadjusted date, so only
//! the 30/360 family is admitted. Read from settlement, a 30/360 US yield can count a
//! 31st one day later than the 30th; the tail keeps whole periods. A reset coupon not yet fixed holds the
//! benchmark's current level, and the quote carries that level and each reset's origin.
use super::workout::{
    charge_cash_evaluations, check_information, check_purchase, eligible_workout_paths,
    PreparedWorkout,
};
use super::*;
use convex_bonds::contracts::{
    BondCalendarIds, BondCalendars, BondContract, BondTerms, PerpetualContract,
};
use convex_core::{
    calendars::Calendar,
    daycounts::DayCountConvention,
    fixings::{FixingContext, FixingSeries, ResolvedRate},
    types::Compounding,
};
use convex_curves::contracts::{discount_from_rate, year_fraction};
use rust_decimal::Decimal;
use serde::Serialize;

/// The lowest yield a perpetuity is solved at. Its price grows without bound as the yield
/// falls to zero, and the tail diverges at zero and below.
pub const MIN_PERPETUITY_YIELD: f64 = 1e-4;

/// The benchmark a reset perpetual's coupon resets to.
pub struct ResetMarket<'a> {
    /// The benchmark's published fixings.
    pub fixings: &'a FixingSeries,
    /// When each fixing is due to publish.
    pub publications: &'a PublicationSchedule,
    /// The fixings' information cutoff, and their valuation date: the quote's reference
    /// date.
    pub fixing: FixingContext,
    /// The benchmark's current level, held for every reset not yet fixed.
    pub held_level: Decimal,
}

/// A perpetual, its purchase, and how far its calls are quoted.
pub struct PerpetualInputs<'a> {
    /// The checked perpetual.
    pub contract: &'a PerpetualContract,
    /// Information, purchase and notice state.
    pub context: &'a WorkoutContext,
    /// The calendar identities `calendars` are bound to: the contract's own.
    pub calendar_ids: &'a BondCalendarIds,
    /// The calendars bound to those identities.
    pub calendars: &'a BondCalendars<'a>,
    /// The last call date quoted.
    pub horizon: Date,
    /// The reset benchmark, for a coupon that resets.
    pub reset_market: Option<ResetMarket<'a>>,
}

/// How a perpetual is redeemed on one quoted path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PerpetualWorkout {
    /// Called on this date.
    Call {
        /// The call date, not its later cash date.
        date: Date,
    },
    /// Never called.
    Perpetuity,
}

/// One reset as the quote applies it.
#[derive(Debug, Clone, Serialize)]
pub struct AppliedReset {
    /// The coupon date the rate applies from.
    pub reset_date: Date,
    /// The benchmark observation and rate: published, or projected at the held level.
    pub benchmark: ResolvedRate,
    /// Annualized decimal coupon: the benchmark plus the margin.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub coupon_rate: Decimal,
}

/// The perpetuity's cash after its last explicit coupon.
#[derive(Debug, Clone, Serialize)]
pub struct PerpetuityTail {
    /// The coupon date the tail follows.
    pub start: Date,
    /// Annualized decimal coupon rate in force for ever.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub coupon_rate: Decimal,
    /// The cash each period pays.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub coupon_amount: Decimal,
}

/// What every quote of a perpetual is composed from.
#[derive(Debug, Clone, Serialize)]
pub struct PerpetualComposition {
    /// The full perpetual terms.
    pub source: BondTerms,
    /// Information, purchase and notice state.
    pub context: WorkoutContext,
    /// The last call date quoted.
    pub horizon: Date,
    /// The call dates quoted, after the notice rules.
    pub calls: Vec<Date>,
    /// True when a supplied notice forces its call, and no other path is quoted.
    pub forced_by_notice: bool,
    /// Each reset before the tail and the dated composition's end, fixed or held.
    pub resets: Vec<AppliedReset>,
    /// The benchmark level every reset not yet fixed holds; absent for a fixed coupon.
    #[serde(serialize_with = "rust_decimal::serde::str_option::serialize")]
    pub held_level: Option<Decimal>,
    /// The perpetuity's cash after its last explicit coupon.
    pub tail: PerpetuityTail,
}

/// One path's yield from the common price target.
#[derive(Debug, Clone, Serialize)]
pub struct PerpetualYield {
    /// The path.
    pub workout: PerpetualWorkout,
    /// A call's dates and redemption cash.
    pub exercise: Option<WorkoutCashDetails>,
    /// Annualized decimal yield under the common convention.
    pub decimal_yield: f64,
    /// Signed dirty-price residual at the yield.
    pub native_residual: f64,
    /// Clean price per 100 face.
    pub clean_per100: f64,
    /// Dirty price per 100 face.
    pub dirty_per100: f64,
    /// Purchase accrued per 100 face.
    pub accrued_per100: f64,
    /// Solver iterations.
    pub iterations: u32,
    /// Price evaluations.
    pub price_evaluations: usize,
}

/// Each path's yield from one price, and the worst (lowest) of them.
#[derive(Debug, Clone, Serialize)]
pub struct PerpetualYieldToWorst {
    /// What the quotes are composed from.
    pub composition: PerpetualComposition,
    /// The common dated yield convention.
    pub convention: DatedYieldConvention,
    /// The clean or dirty price target per 100 face.
    pub target: BondPriceTarget,
    /// The inverse bounds and work budgets enforced.
    pub settings: WorkoutSolveSettings,
    /// Each call in date order, then the perpetuity.
    pub quotes: Vec<PerpetualYield>,
    /// The lowest yield.
    pub selected_index: usize,
    /// Price evaluations across every path.
    pub price_evaluations: usize,
    /// Cash discount evaluations across every path.
    pub cash_discount_evaluations: u64,
    /// The work reserved before any path was solved.
    pub reserved_cash_evaluations: u64,
}

/// One path's price at the common yield.
#[derive(Debug, Clone, Serialize)]
pub struct PerpetualPrice {
    /// The path.
    pub workout: PerpetualWorkout,
    /// A call's dates and redemption cash.
    pub exercise: Option<WorkoutCashDetails>,
    /// Clean price per 100 face.
    pub clean_per100: f64,
    /// Dirty price per 100 face.
    pub dirty_per100: f64,
    /// Purchase accrued per 100 face.
    pub accrued_per100: f64,
}

/// Each path's price at one yield, and the worst (lowest) of them.
#[derive(Debug, Clone, Serialize)]
pub struct PerpetualPriceToWorst {
    /// What the quotes are composed from.
    pub composition: PerpetualComposition,
    /// The common dated yield convention.
    pub convention: DatedYieldConvention,
    /// The annualized decimal yield every path is priced at.
    pub decimal_yield: f64,
    /// Each call in date order, then the perpetuity when the yield is above zero.
    pub quotes: Vec<PerpetualPrice>,
    /// The lowest price.
    pub selected_index: usize,
}

struct PreparedPerpetual {
    composition: PerpetualComposition,
    dated: BondContract,
    /// The most cash flows any path has.
    max_cash: usize,
    perpetuity: PreparedBondCash,
}

/// Every call date up to the horizon and the perpetuity, each solved from one price; the
/// worst is the lowest yield. A notice already given forces its call alone.
pub fn quote_perpetual_worst(
    input: &PerpetualInputs<'_>,
    convention: DatedYieldConvention,
    target: BondPriceTarget,
    settings: WorkoutSolveSettings,
) -> WorkflowResult<PerpetualYieldToWorst> {
    settings.check_budgets()?;
    let prepared = prepare(input, convention, settings.max_dates)?;
    let calls = &prepared.composition.calls;
    let paths = calls.len() + usize::from(!prepared.composition.forced_by_notice);
    let reserved = settings.reserve_cash_evaluations(paths, prepared.max_cash)?;
    let mut quotes = Vec::with_capacity(paths);
    let mut cash_evaluations = 0u64;
    let mut record = |workout, exercise, inverse: BondInverse| -> WorkflowResult<()> {
        charge_cash_evaluations(&mut cash_evaluations, &inverse)?;
        quotes.push(PerpetualYield {
            workout,
            exercise,
            decimal_yield: inverse.decimal_parameter,
            native_residual: inverse.native_residual,
            clean_per100: inverse.valuation.clean_per100,
            dirty_per100: inverse.valuation.dirty_per100,
            accrued_per100: inverse.valuation.accrued_per100,
            iterations: inverse.iterations,
            price_evaluations: inverse.price_evaluations,
        });
        Ok(())
    };
    for &date in calls {
        let path = prepared.call(input, date)?;
        let inverse = solve_bond_yield(&path.cash, convention, target, settings.inverse)?;
        record(PerpetualWorkout::Call { date }, path.exercise, inverse)?;
    }
    if !prepared.composition.forced_by_notice {
        let bounds = BondInverseSettings {
            lower: settings.inverse.lower.max(MIN_PERPETUITY_YIELD),
            ..settings.inverse
        };
        // The tail's coupon is positive, so the price falls with the yield even when the
        // explicit cash holds nothing after settlement.
        let inverse = solve_bond(&prepared.perpetuity, true, target, bounds, 10.0, |y| {
            prepared.perpetuity_value(y, convention)
        })?;
        record(PerpetualWorkout::Perpetuity, None, inverse)?;
    }
    if cash_evaluations > reserved {
        return Err(CashflowError::BoundExceeded("workout reserved cash evaluations").into());
    }
    let selected_index = lowest(quotes.iter().map(|q| q.decimal_yield))?;
    Ok(PerpetualYieldToWorst {
        composition: prepared.composition,
        convention,
        target,
        settings,
        price_evaluations: quotes.iter().map(|q| q.price_evaluations).sum(),
        quotes,
        selected_index,
        cash_discount_evaluations: cash_evaluations,
        reserved_cash_evaluations: reserved,
    })
}

/// Every call date up to the horizon and the perpetuity, each priced at one yield; the
/// worst is the lowest price. The perpetuity is priced only at a yield above zero. A
/// notice already given forces its call alone.
pub fn quote_perpetual_price_to_worst(
    input: &PerpetualInputs<'_>,
    convention: DatedYieldConvention,
    decimal_yield: f64,
) -> WorkflowResult<PerpetualPriceToWorst> {
    let prepared = prepare(input, convention, MAX_WORKOUT_DATES)?;
    let mut quotes = Vec::with_capacity(prepared.composition.calls.len() + 1);
    let mut push = |workout, exercise, value: BondValue| {
        quotes.push(PerpetualPrice {
            workout,
            exercise,
            clean_per100: value.clean_per100,
            dirty_per100: value.dirty_per100,
            accrued_per100: value.accrued_per100,
        });
    };
    for &date in &prepared.composition.calls {
        let path = prepared.call(input, date)?;
        let value = path.cash.price_yield(decimal_yield, convention)?;
        push(PerpetualWorkout::Call { date }, path.exercise, value);
    }
    if !prepared.composition.forced_by_notice && decimal_yield > 0.0 {
        let value = prepared.perpetuity_value(decimal_yield, convention)?;
        push(PerpetualWorkout::Perpetuity, None, value);
    }
    let selected_index = lowest(quotes.iter().map(|q| q.dirty_per100))?;
    Ok(PerpetualPriceToWorst {
        composition: prepared.composition,
        convention,
        decimal_yield,
        quotes,
        selected_index,
    })
}

fn lowest(values: impl Iterator<Item = f64>) -> WorkflowResult<usize> {
    values
        .enumerate()
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(i, _)| i)
        .ok_or_else(|| invalid("perpetual.selection", "no call or perpetuity quoted"))
}

fn next_day(date: Date, field: &'static str) -> WorkflowResult<Date> {
    date.checked_add_days(1)
        .map_err(|e| invalid(field, e.to_string()))
}

fn prepare(
    input: &PerpetualInputs<'_>,
    convention: DatedYieldConvention,
    max_dates: u32,
) -> WorkflowResult<PreparedPerpetual> {
    let contract = input.contract;
    let terms = contract.terms();
    let c = input.context;
    // The yield's day count is also one a dated yield reads.
    if !matches!(
        convention.day_count,
        DayCountConvention::Thirty360US | DayCountConvention::Thirty360E
    ) || convention.compounding == Compounding::Simple
    {
        return Err(WorkflowError::Unsupported(
            "a perpetuity's tail counts each period as 1/f year: a 30/360 US or 30E/360 yield, compounded",
        ));
    }
    if input.calendar_ids != &terms.calendars {
        return Err(invalid(
            "perpetual.calendars",
            "calendars bound to the contract's own calendar identities",
        ));
    }
    check_purchase(
        terms,
        c,
        (&input.calendar_ids.payment, input.calendars.payment),
    )?;
    check_information(terms, c)?;
    if input.horizon <= c.settlement {
        return Err(invalid("perpetual.horizon", "after settlement"));
    }
    let first_call = contract.first_call();
    // The first coupon date on or after the first call that ends settlement's period.
    let mut tail_start = contract
        .coupon_date_on_or_after(next_day(c.settlement, "perpetual.settlement")?.max(first_call))?;
    // The dated composition runs past every call quoted.
    let mut end = contract.coupon_date_on_or_after(next_day(
        input.horizon.max(first_call),
        "perpetual.horizon",
    )?)?;
    let (resets, held_level) = match (&contract.perpetual().reset, &input.reset_market) {
        (None, None) => (vec![], None),
        (Some(_), Some(market)) => {
            if market.fixing.valuation_date != c.reference_date {
                return Err(invalid(
                    "perpetual.reset_market",
                    "fixings read as of the quote's information date",
                ));
            }
            // The tail follows the last reset fixed by the valuation date: every later one
            // holds the current level.
            if let Some(first_held) = contract
                .first_reset_fixed_after(market.fixing.valuation_date, input.calendars.reset)?
            {
                tail_start = tail_start.max(first_held.reset_date);
            }
            end = end.max(tail_start);
            (
                applied_resets(contract, market, end, input.calendars.reset)?,
                Some(market.held_level),
            )
        }
        (Some(_), None) => {
            return Err(invalid(
                "perpetual.reset_market",
                "a reset coupon needs its benchmark's fixings and held level",
            ))
        }
        (None, Some(_)) => {
            return Err(invalid(
                "perpetual.reset_market",
                "the coupon does not reset; no benchmark is read",
            ))
        }
    };
    let rates: Vec<Decimal> = resets.iter().map(|r| r.benchmark.rate).collect();
    let dated = contract.dated_through(end, input.horizon, &rates, input.calendars)?;
    let (calls, forced_by_notice) = quoted_calls(input, &dated, max_dates)?;
    let tail_rate = contract.perpetual_coupon_rate(held_level)?;
    let tail_coupon = contract.regular_coupon(tail_rate)?;
    if tail_coupon <= Decimal::ZERO {
        return Err(WorkflowError::Unsupported(
            "a perpetuity needs a positive coupon",
        ));
    }
    let mut cash = dated.fixed_projection(c.settlement, c.on_payment)?;
    let max_cash = cash.cashflows()?.len();
    cash.coupons
        .retain(|coupon| coupon.contractual_end <= tail_start);
    cash.principal.clear();
    let perpetuity = PreparedBondCash::workout(&dated, c.settlement, cash);
    Ok(PreparedPerpetual {
        composition: PerpetualComposition {
            source: terms.clone(),
            context: c.clone(),
            horizon: input.horizon,
            calls,
            forced_by_notice,
            resets,
            held_level,
            tail: PerpetuityTail {
                start: tail_start,
                coupon_rate: tail_rate,
                coupon_amount: tail_coupon,
            },
        },
        dated,
        max_cash,
        perpetuity,
    })
}

/// Each reset before `end`: its published fixing, or the held level after the valuation
/// date. A fixing due by then and not published is refused.
fn applied_resets(
    contract: &PerpetualContract,
    market: &ResetMarket<'_>,
    end: Date,
    reset_calendar: &dyn Calendar,
) -> WorkflowResult<Vec<AppliedReset>> {
    let reset = contract
        .perpetual()
        .reset
        .as_ref()
        .ok_or_else(|| invalid("perpetual.reset_market", "the coupon does not reset"))?;
    let dates = contract.resets(end, reset_calendar)?;
    let requirements = dates
        .iter()
        .map(|d| {
            Ok(FixingRequirement {
                index_id: reset.benchmark_id.clone(),
                observation_date: d.fixing_date,
                expected_publication: market
                    .publications
                    .get(&reset.benchmark_id, d.fixing_date)?,
            })
        })
        .collect::<WorkflowResult<Vec<_>>>()?;
    let rates = market
        .fixings
        .resolve(&requirements, market.fixing)?
        .project(|_| Ok(market.held_level))?;
    dates
        .iter()
        .zip(rates)
        .map(|(d, benchmark)| {
            Ok(AppliedReset {
                reset_date: d.reset_date,
                coupon_rate: contract.reset_coupon_rate(benchmark.rate)?,
                benchmark,
            })
        })
        .collect()
}

fn workout_inputs<'a>(
    input: &'a PerpetualInputs<'a>,
    dated: &'a BondContract,
) -> WorkoutInputs<'a> {
    WorkoutInputs {
        contract: dated,
        context: input.context,
        exercise_calendar: (&input.calendar_ids.exercise, input.calendars.exercise),
        purchase_calendar: (&input.calendar_ids.payment, input.calendars.payment),
    }
}

/// The call dates after settlement up to the horizon that the notice rules leave, or the
/// one a notice already given forces.
fn quoted_calls(
    input: &PerpetualInputs<'_>,
    dated: &BondContract,
    max_dates: u32,
) -> WorkflowResult<(Vec<Date>, bool)> {
    let (paths, _, forced) = eligible_workout_paths(&workout_inputs(input, dated), max_dates)?;
    let calls: Vec<Date> = paths
        .into_iter()
        .filter_map(|path| match path {
            WorkoutSelection::Exercise { date, .. } => Some(date),
            WorkoutSelection::Maturity {} => None,
        })
        .collect();

    Ok((calls, forced))
}

impl PreparedPerpetual {
    fn call(&self, input: &PerpetualInputs<'_>, date: Date) -> WorkflowResult<PreparedWorkout> {
        super::workout::prepare(
            &self.dated,
            input.context,
            &WorkoutSelection::Exercise { provision: 0, date },
            &input.calendar_ids.exercise,
            input.calendars.exercise,
        )
    }

    /// The perpetuity's price: its explicit cash at the dated yield, plus the tail in
    /// closed form from its start.
    fn perpetuity_value(
        &self,
        decimal_yield: f64,
        convention: DatedYieldConvention,
    ) -> WorkflowResult<BondValue> {
        if decimal_yield <= 0.0 {
            return Err(WorkflowError::NotApplicable(
                "a perpetuity has a price only at a yield above zero",
            ));
        }
        let mut value = self.perpetuity.price_yield(decimal_yield, convention)?;
        let tail = &self.composition.tail;
        let periods = f64::from(
            self.composition
                .source
                .schedule
                .frequency
                .periods_per_year(),
        );
        let one_period = discount_from_rate(decimal_yield, 1.0 / periods, convention.compounding)?;
        let to_start = discount_from_rate(
            decimal_yield,
            year_fraction(
                self.perpetuity.settlement(),
                tail.start,
                convention.day_count,
            )?,
            convention.compounding,
        )?;
        let face = decimal_to_f64(self.perpetuity.face_value(), "face")?;
        let tail_per100 = finite(
            decimal_to_f64(tail.coupon_amount, "perpetuity tail")? / face
                * 100.0
                * to_start
                * one_period
                / (1.0 - one_period),
            "perpetuity tail",
        )?;
        value.dirty_per100 = finite(value.dirty_per100 + tail_per100, "dirty price")?;
        value.clean_per100 = finite(value.clean_per100 + tail_per100, "clean price")?;
        value.present_value = crate::quote_risk::risk_reference_pv(
            value.dirty_per100 / 100.0 * face,
            self.perpetuity.currency(),
        )?;
        Ok(value)
    }
}
