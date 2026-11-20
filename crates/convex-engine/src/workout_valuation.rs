//! Callables on their workouts: a callable quoted by price or by its yield to worst is
//! valued on each call date and maturity, as high-yield callables are quoted, with no
//! option model and no curve. From a clean price the model solves the yield to worst;
//! from a yield to worst it prices to worst. The yields are dated yields under the bond's
//! own day count, compounded at its coupon frequency, which for a 30/360 or ACT/ACT ISDA
//! bond is its street yield. The calculator also reports each workout's yield. Every
//! formula is the pure crates' (`convex_analytics::workflow`): the engine builds the
//! contract and its context only.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_analytics::workflow::{
    quote_workout_price_to_worst, quote_workout_worst, BondInverseSettings, BondPriceTarget,
    DatedYieldConvention, WorkflowError, WorkoutContext, WorkoutInputs, WorkoutNotice,
    WorkoutPriceToWorst, WorkoutSelection, WorkoutSolveSettings, WorkoutWorstQuotation,
    MAX_WORKOUT_CASH_EVALUATIONS,
};
use convex_bonds::contracts::BondContract;
use convex_core::{
    calendars::Calendar,
    cashflows::{latest_trade_date, PaymentDateEntitlement},
    contracts::ErrorCode,
    types::Compounding,
    Date,
};
use rust_decimal::Decimal;

use crate::level_pricing::{check_level, LevelError};
use crate::optional_valuation::{
    calendar_window, callable_terms, failure, library_failure, stored_call_terms, to_f64, Failure,
};
use crate::ports::{
    level_pricing::{
        is_workout_callable, PricingMethod, MAX_WORKOUT_CALLS, NOT_A_WORKOUT_CALLABLE,
    },
    output::BondQuoteOutput,
    reference_data::BondReferenceData,
    reference_master::{CalendarDefinition, ResolvedBondReference},
};
use crate::pricing_router::PricingRouter;

/// Quantitative model for callables quoted by price or yield to worst: dated yields on
/// each workout.
pub const WORKOUT_SNAPSHOT_MODEL: &str = "convex-callable-workout-v1";
/// Measures a callable on its workouts publishes.
pub const WORKOUT_MEASURES: [&str; 5] = [
    "clean_price_mid",
    "dirty_price_mid",
    "accrued_interest",
    "ytw_mid",
    "years_to_worst_mid",
];

// Every path's yield lies in this closed domain: a premium bond's yield to a near call can
// be far below zero, and a distressed bond's far above. At most one examined date a call.
const WORST_SOLVE: WorkoutSolveSettings = WorkoutSolveSettings {
    inverse: BondInverseSettings {
        lower: -0.9,
        upper: 5.0,
        price_tolerance: 1e-9,
        parameter_tolerance: 1e-13,
        max_iterations: 128,
    },
    max_dates: MAX_WORKOUT_CALLS as u32,
    max_cash_evaluations: MAX_WORKOUT_CASH_EVALUATIONS,
};

/// What a callable on its workouts is quoted by.
#[derive(Debug, Clone, Copy)]
pub(crate) enum WorkoutQuote {
    /// A clean price per 100 face.
    CleanPrice(Decimal),
    /// A decimal yield to worst.
    YieldToWorst(Decimal),
}

impl WorkoutQuote {
    /// `level` as `method` quotes a callable on its workouts: a clean price, or a yield to
    /// worst within the range a rule's yield level takes. A spread does not quote one.
    pub(crate) fn quoted_by(method: &PricingMethod, level: Decimal) -> Result<Self, LevelError> {
        match method {
            PricingMethod::Price {} => Ok(Self::CleanPrice(level)),
            PricingMethod::Yield {} => {
                check_level(method, level)?;
                Ok(Self::YieldToWorst(level))
            }
            PricingMethod::ZSpread {}
            | PricingMethod::BenchmarkSpread { .. }
            | PricingMethod::DiscountMargin {} => Err(LevelError::Refused(format!(
                "a callable is valued on its workouts from a price or a yield to worst, not a {}",
                method.name()
            ))),
        }
    }
}

/// A callable valued on its workouts.
#[derive(Debug, Clone)]
pub struct WorkoutValuation {
    /// Clean price per 100 face: the quoted price, or the price to worst.
    pub clean_price: Decimal,
    /// Dirty price per 100 face on the worst path.
    pub dirty_price: f64,
    /// Accrued per 100 face at settlement.
    pub accrued: f64,
    /// The lowest yield among the calls and maturity, as a decimal.
    pub yield_to_worst: f64,
    /// Years from settlement to the worst date, under the bond's day count.
    pub years_to_worst: f64,
}

/// How a callable is redeemed on a workout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Redemption {
    /// At maturity.
    Maturity,
    /// At an issuer call.
    Call {
        /// The call price per 100 face.
        call_price: Decimal,
    },
}

/// One date a callable can be worked out to, with its yield at the calculated price.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Workout {
    /// Maturity, or a call at its price.
    pub redemption: Redemption,
    /// The maturity date, or the call's contractual exercise date.
    pub date: Date,
    /// The yield to this date at the calculated clean price, as a decimal.
    pub decimal_yield: f64,
}

/// A callable calculated on its workouts: its valuation and each workout's yield.
#[derive(Debug, Clone)]
pub struct WorkoutCalculation {
    /// The valuation, as Live and request-time quotes value the callable.
    pub valuation: WorkoutValuation,
    /// Each call whose notice can still be given, then maturity, in date order.
    pub workouts: Vec<Workout>,
    /// The workout of lowest yield, on which the callable is valued.
    pub worst: Workout,
}

impl WorkoutCalculation {
    /// The calculation as a quote of `bond` at `settlement`: the clean price, accrued and
    /// yield to worst, as a request-time quote reads them from the workout model.
    pub fn bond_quote(&self, bond: &BondReferenceData, settlement: Date) -> BondQuoteOutput {
        let valuation = &self.valuation;
        BondQuoteOutput {
            isin: bond.isin.clone(),
            clean_price_mid: Some(valuation.clean_price),
            accrued_interest: Decimal::from_f64_retain(valuation.accrued),
            ytw: Decimal::from_f64_retain(valuation.yield_to_worst),
            ..BondQuoteOutput::without_measures(
                bond.instrument_id.clone(),
                bond.currency,
                settlement,
                chrono::Utc::now().timestamp_millis(),
                WORKOUT_SNAPSHOT_MODEL.into(),
                "convex-library".into(),
            )
        }
    }
}

/// Calculate a stored callable on its workouts from `level`, quoted by `method` whatever
/// its rule quotes: its valuation, as Live and request-time quotes give it, and each
/// workout's yield at its clean price. From a yield to worst, the yields are solved at the
/// price to worst, so the worst's yield is the level to the solver's tolerance.
pub fn calculate_on_workouts(
    reference: &ResolvedBondReference,
    method: &PricingMethod,
    level: Decimal,
    settlement: Date,
) -> Result<WorkoutCalculation, LevelError> {
    let quote = WorkoutQuote::quoted_by(method, level)?;
    let refused = |reason: &str| LevelError::Refused(reason.into());
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or_else(|| refused("the stored reference is deleted"))?;
    let calendar = reference
        .calendar_definition()
        .ok_or_else(|| refused("the pinned calendar resource is not a calendar"))?;
    calculate(bond, calendar, settlement, quote).map_err(|failure| match failure.code {
        ErrorCode::InvalidInput => LevelError::Refused(failure.message),
        ErrorCode::UnsupportedConvention => LevelError::Unsupported(failure.message),
        _ => LevelError::Analytics(failure.message),
    })
}

fn calculate(
    bond: &BondReferenceData,
    calendar: &CalendarDefinition,
    settlement: Date,
    quote: WorkoutQuote,
) -> Result<WorkoutCalculation, Failure> {
    let workouts = StoredWorkouts::new(bond, calendar, settlement)?;
    let (valuation, yields, worst_index) = match quote {
        WorkoutQuote::CleanPrice(clean) => {
            let yields = workouts.yields_at_price(clean)?;
            let valuation = workouts.valuation_from_price(clean, &yields)?;
            let worst_index = yields.selected_index;
            (valuation, yields, worst_index)
        }
        WorkoutQuote::YieldToWorst(level) => {
            let prices = workouts.prices_at_yield(level)?;
            let valuation = workouts.valuation_from_yield(&prices)?;
            // The same paths in the same order, so the worst is the path priced to.
            let yields = workouts.yields_at_price(valuation.clean_price)?;
            (valuation, yields, prices.selected_index)
        }
    };
    let mut table = yields
        .quotes
        .iter()
        .map(|path| workouts.workout(&path.selection, path.decimal_yield))
        .collect::<Result<Vec<_>, _>>()?;
    let worst = *table
        .get(worst_index)
        .ok_or_else(|| invalid("the worst is not among the workouts".into()))?;
    // Maturity is quoted first and falls after every call.
    table.sort_by_key(|workout| workout.date);
    Ok(WorkoutCalculation {
        valuation,
        workouts: table,
        worst,
    })
}

/// Value a stored callable on its workouts at `settlement`. The calls considered are the
/// ones whose notice can still be given on or after the trade date that settles then: no
/// notice is stored, so no call is taken as already noticed.
pub(crate) fn value_on_workouts(
    bond: &BondReferenceData,
    calendar: &CalendarDefinition,
    settlement: Date,
    quote: WorkoutQuote,
) -> Result<WorkoutValuation, Failure> {
    let workouts = StoredWorkouts::new(bond, calendar, settlement)?;
    match quote {
        WorkoutQuote::CleanPrice(clean) => {
            workouts.valuation_from_price(clean, &workouts.yields_at_price(clean)?)
        }
        WorkoutQuote::YieldToWorst(level) => {
            workouts.valuation_from_yield(&workouts.prices_at_yield(level)?)
        }
    }
}

/// A stored callable's contract, bought at one settlement, as the workout operations take
/// it.
struct StoredWorkouts {
    contract: BondContract,
    /// The router builds a stored bond on one calendar for payment, reset, observation and
    /// exercise.
    calendar: Box<dyn Calendar>,
    context: WorkoutContext,
    convention: DatedYieldConvention,
}

impl StoredWorkouts {
    fn new(
        bond: &BondReferenceData,
        calendar: &CalendarDefinition,
        settlement: Date,
    ) -> Result<Self, Failure> {
        if !is_workout_callable(bond) {
            return Err(failure(
                ErrorCode::UnsupportedConvention,
                NOT_A_WORKOUT_CALLABLE,
            ));
        }
        let (terms, call) = stored_call_terms(bond)?;
        calendar_window(bond, terms, calendar)?;
        let base = PricingRouter { bump_bps: 1.0 }
            .to_fixed_rate_bond(bond)
            .map_err(|e| invalid(e.to_string()))?;
        let contract = callable_terms(base, bond, call)?
            .build()
            .map_err(|e| invalid(e.to_string()))?;
        let source = contract.terms();
        let calendar = source
            .calendars
            .payment
            .try_to_calendar()
            .map_err(|e| invalid(e.to_string()))?;
        let trade = latest_trade_date(
            settlement,
            source.settlement_lag,
            source.settlement_adjustment,
            calendar.as_ref(),
        )
        .map_err(|e| invalid(e.to_string()))?
        .ok_or_else(|| {
            invalid(format!(
                "no trade settles on {settlement} under the bond's settlement lag and calendar"
            ))
        })?;
        let context = WorkoutContext {
            reference_date: trade,
            trade_date: trade,
            settlement,
            on_payment: PaymentDateEntitlement::Exclude,
            notice: WorkoutNotice::Unnotified {
                earliest_notice: trade,
            },
        };
        let convention = DatedYieldConvention {
            day_count: source.day_count,
            compounding: Compounding::from(source.schedule.frequency),
        };
        Ok(Self {
            contract,
            calendar,
            context,
            convention,
        })
    }

    fn inputs(&self) -> WorkoutInputs<'_> {
        let calendars = &self.contract.terms().calendars;
        WorkoutInputs {
            contract: &self.contract,
            context: &self.context,
            exercise_calendar: (&calendars.exercise, self.calendar.as_ref()),
            purchase_calendar: (&calendars.payment, self.calendar.as_ref()),
        }
    }

    /// Every workout's yield at `clean`, and the worst.
    fn yields_at_price(&self, clean: Decimal) -> Result<WorkoutWorstQuotation, Failure> {
        let target = BondPriceTarget::Clean(to_f64(clean, "clean price")?);
        quote_workout_worst(&self.inputs(), self.convention, target, WORST_SOLVE)
            .map_err(workflow_failure)
    }

    /// Every workout's price at `yield_to_worst`, and the lowest.
    fn prices_at_yield(&self, yield_to_worst: Decimal) -> Result<WorkoutPriceToWorst, Failure> {
        let level = to_f64(yield_to_worst, "yield")?;
        quote_workout_price_to_worst(
            &self.inputs(),
            self.convention,
            level,
            WORST_SOLVE.max_dates,
        )
        .map_err(workflow_failure)
    }

    /// The valuation at a quoted clean price, from its workouts' yields.
    fn valuation_from_price(
        &self,
        clean: Decimal,
        yields: &WorkoutWorstQuotation,
    ) -> Result<WorkoutValuation, Failure> {
        let path = &yields.quotes[yields.selected_index];
        Ok(WorkoutValuation {
            clean_price: clean,
            dirty_price: path.dirty_per100,
            accrued: path.accrued_per100,
            yield_to_worst: path.decimal_yield,
            years_to_worst: self.years_to(&path.selection)?,
        })
    }

    /// The valuation at a quoted yield to worst, from its workouts' prices.
    fn valuation_from_yield(
        &self,
        prices: &WorkoutPriceToWorst,
    ) -> Result<WorkoutValuation, Failure> {
        let path = &prices.quotes[prices.selected_index];
        Ok(WorkoutValuation {
            clean_price: Decimal::from_f64_retain(path.clean_per100)
                .ok_or_else(|| invalid("the price to worst is not finite".into()))?,
            dirty_price: path.dirty_per100,
            accrued: path.accrued_per100,
            yield_to_worst: prices.decimal_yield,
            years_to_worst: self.years_to(&path.selection)?,
        })
    }

    /// The date a workout redeems the bond on.
    fn date_of(&self, selection: &WorkoutSelection) -> Date {
        match selection {
            WorkoutSelection::Maturity {} => self.contract.terms().schedule.end_date,
            WorkoutSelection::Exercise { date, .. } => *date,
        }
    }

    /// Years from settlement to a workout, under the bond's day count.
    fn years_to(&self, selection: &WorkoutSelection) -> Result<f64, Failure> {
        convex_curves::contracts::year_fraction(
            self.context.settlement,
            self.date_of(selection),
            self.contract.terms().day_count,
        )
        .map_err(|e| invalid(e.to_string()))
    }

    /// A workout of the table: its redemption, its date and its yield.
    fn workout(
        &self,
        selection: &WorkoutSelection,
        decimal_yield: f64,
    ) -> Result<Workout, Failure> {
        let redemption = match selection {
            WorkoutSelection::Maturity {} => Redemption::Maturity,
            WorkoutSelection::Exercise { provision, .. } => Redemption::Call {
                call_price: self
                    .contract
                    .terms()
                    .exercise
                    .get(*provision as usize)
                    .ok_or_else(|| invalid("a workout names no call of the bond".into()))?
                    .price_per_100,
            },
        };
        Ok(Workout {
            redemption,
            date: self.date_of(selection),
            decimal_yield,
        })
    }
}

fn invalid(reason: String) -> Failure {
    failure(ErrorCode::InvalidInput, reason)
}

fn workflow_failure(error: WorkflowError) -> Failure {
    library_failure(error.into())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ports::{
        reference_data::CallScheduleEntry,
        reference_master::{CallStyle, MasterDefinition},
    };
    use convex_analytics::{bond_quote::price_from_level, workflow::WorkoutValue};
    use convex_core::types::{Frequency, Mark, PriceKind};
    use rust_decimal::prelude::ToPrimitive;
    use std::str::FromStr;

    const SETTLEMENT: &str = "2026-10-06";

    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }
    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).unwrap()
    }
    /// The fixture 5.375% 2030 bond, callable at 101, 100.5 and par in January 2027, 2028
    /// and 2029, on 30 days' notice.
    fn callable(style: CallStyle) -> BondReferenceData {
        crate::products::callable(
            serde_json::from_str(include_str!(
                "../../convex-ports/tests/fixtures/reference_bond.json"
            ))
            .unwrap(),
            style,
        )
    }
    /// The calendar the fixture bond's reference pins.
    fn fixture_calendar(bond: &BondReferenceData) -> CalendarDefinition {
        let resources = crate::reference_terms::resources(bond);
        let MasterDefinition::Calendar(calendar) = &resources[1].definition else {
            panic!("fixture calendar resource")
        };
        (**calendar).clone()
    }
    fn value(
        bond: &BondReferenceData,
        settlement: &str,
        quote: WorkoutQuote,
    ) -> Result<WorkoutValuation, Failure> {
        value_on_workouts(bond, &fixture_calendar(bond), date(settlement), quote)
    }
    fn calculated(
        bond: &BondReferenceData,
        settlement: &str,
        quote: WorkoutQuote,
    ) -> WorkoutCalculation {
        calculate(bond, &fixture_calendar(bond), date(settlement), quote).unwrap()
    }

    #[test]
    fn the_yield_to_worst_is_the_library_worst_on_the_stored_contract() {
        let bond = callable(CallStyle::Bermudan);
        let ours = value(
            &bond,
            SETTLEMENT,
            WorkoutQuote::CleanPrice(decimal("101.25")),
        )
        .unwrap();
        // The same contract and context, built here without the engine.
        let base = PricingRouter { bump_bps: 1.0 }
            .to_fixed_rate_bond(&bond)
            .unwrap();
        let mut schedule =
            convex_bonds::types::CallSchedule::new(convex_bonds::types::CallType::Bermudan);
        for entry in &bond.call_schedule {
            schedule.entries.push(convex_bonds::types::CallEntry {
                start_date: entry.call_date,
                end_date: None,
                call_price: entry.call_price.to_f64().unwrap(),
                notice_days: Some(30),
            });
        }
        let contract = convex_bonds::instruments::CallableBond::new(base, schedule)
            .contract_terms()
            .unwrap()
            .build()
            .unwrap();
        let sifma = contract
            .terms()
            .calendars
            .payment
            .try_to_calendar()
            .unwrap();
        // A Tuesday settles the Friday before's trade.
        let trade = date("2026-10-02");
        let context = WorkoutContext {
            reference_date: trade,
            trade_date: trade,
            settlement: date(SETTLEMENT),
            on_payment: PaymentDateEntitlement::Exclude,
            notice: WorkoutNotice::Unnotified {
                earliest_notice: trade,
            },
        };
        let inputs = WorkoutInputs {
            contract: &contract,
            context: &context,
            exercise_calendar: (&contract.terms().calendars.exercise, sifma.as_ref()),
            purchase_calendar: (&contract.terms().calendars.payment, sifma.as_ref()),
        };
        let convention = DatedYieldConvention {
            day_count: convex_core::daycounts::DayCountConvention::ActActIsda,
            compounding: Compounding::SemiAnnual,
        };
        let worst = quote_workout_worst(
            &inputs,
            convention,
            BondPriceTarget::Clean(101.25),
            WORST_SOLVE,
        )
        .unwrap();
        // Maturity and the three calls; at a premium the first call, at 101, is worst.
        assert_eq!(worst.quotes.len(), 4);
        assert_eq!(
            worst.selected.selection,
            WorkoutSelection::Exercise {
                provision: 0,
                date: date("2027-01-15")
            }
        );
        let WorkoutValue::Yield(selected) = &worst.selected.quotation else {
            panic!("a yield quotation")
        };
        assert_eq!(ours.yield_to_worst, selected.decimal_parameter);
        assert_eq!(ours.accrued, selected.valuation.accrued_per100);
        assert_eq!(ours.dirty_price, selected.valuation.dirty_per100);
        assert_eq!(ours.clean_price, decimal("101.25"));
        // ACT/ACT ISDA from 6 October 2026 to 15 January 2027.
        assert!((ours.years_to_worst - 101.0 / 365.0).abs() < 1e-12);

        // The calculator's table is the library's paths, each at its own yield, in date
        // order: the three calls at their prices, then maturity.
        let table = calculated(
            &bond,
            SETTLEMENT,
            WorkoutQuote::CleanPrice(decimal("101.25")),
        );
        let library = |path: &convex_analytics::workflow::WorkoutYieldSummary| Workout {
            redemption: match path.selection {
                WorkoutSelection::Maturity {} => Redemption::Maturity,
                WorkoutSelection::Exercise { provision, .. } => Redemption::Call {
                    call_price: bond.call_schedule[provision as usize].call_price,
                },
            },
            date: match path.selection {
                WorkoutSelection::Maturity {} => bond.maturity_date,
                WorkoutSelection::Exercise { date, .. } => date,
            },
            decimal_yield: path.decimal_yield,
        };
        let mut expected = worst.quotes.iter().map(library).collect::<Vec<_>>();
        expected.rotate_left(1);
        assert_eq!(table.workouts, expected);
        assert_eq!(
            table
                .workouts
                .iter()
                .map(|w| w.redemption)
                .collect::<Vec<_>>(),
            [
                Redemption::Call {
                    call_price: decimal("101")
                },
                Redemption::Call {
                    call_price: decimal("100.5")
                },
                Redemption::Call {
                    call_price: decimal("100")
                },
                Redemption::Maturity,
            ]
        );
        assert_eq!(table.worst, library(&worst.quotes[worst.selected_index]));
        assert_eq!(table.worst.decimal_yield, ours.yield_to_worst);
        assert_eq!(table.valuation.years_to_worst, ours.years_to_worst);
        assert_eq!(table.valuation.clean_price, ours.clean_price);
    }

    #[test]
    fn at_a_discount_the_worst_is_maturity_and_its_yield_is_the_street_yield() {
        let bond = callable(CallStyle::European);
        let ours = value(&bond, SETTLEMENT, WorkoutQuote::CleanPrice(decimal("95"))).unwrap();
        let street = price_from_level(
            &PricingRouter { bump_bps: 1.0 }
                .to_fixed_rate_bond(&bond)
                .unwrap(),
            date(SETTLEMENT),
            &Mark::Price {
                value: decimal("95"),
                kind: PriceKind::Clean,
            },
            Frequency::SemiAnnual,
            &Default::default(),
        )
        .unwrap();
        assert!((ours.yield_to_worst - street.ytm_decimal).abs() < 1e-10);
        assert!((ours.accrued - street.accrued_per_100).abs() < 1e-12);
        // To 15 January 2030.
        assert!((ours.years_to_worst - (101.0 / 365.0 + 3.0)).abs() < 1e-12);
    }

    #[test]
    fn the_price_to_worst_at_the_yield_to_worst_is_the_price() {
        let bond = callable(CallStyle::Bermudan);
        for clean in ["95", "99.25", "100.75", "101.25", "104"] {
            let from_price =
                value(&bond, SETTLEMENT, WorkoutQuote::CleanPrice(decimal(clean))).unwrap();
            let level = Decimal::from_f64_retain(from_price.yield_to_worst).unwrap();
            let from_yield = value(&bond, SETTLEMENT, WorkoutQuote::YieldToWorst(level)).unwrap();
            assert!(
                (from_yield.clean_price - decimal(clean)).abs() < decimal("0.00000001"),
                "{clean}: {}",
                from_yield.clean_price
            );
            assert_eq!(
                from_yield.years_to_worst, from_price.years_to_worst,
                "{clean}"
            );
            assert!((from_yield.accrued - from_price.accrued).abs() < 1e-12);
        }
    }

    #[test]
    fn from_a_yield_to_worst_the_table_is_solved_at_the_price_to_worst() {
        let bond = callable(CallStyle::Bermudan);
        // At a discount the worst is maturity, at a premium the first call, and between
        // them the last call.
        for clean in ["95", "100.25", "101.25", "104"] {
            let by_price = calculated(&bond, SETTLEMENT, WorkoutQuote::CleanPrice(decimal(clean)));
            let level = Decimal::from_f64_retain(by_price.valuation.yield_to_worst).unwrap();
            let by_yield = calculated(&bond, SETTLEMENT, WorkoutQuote::YieldToWorst(level));
            let valuation = &by_yield.valuation;
            assert!(
                (valuation.clean_price - decimal(clean)).abs() < decimal("0.00000001"),
                "{clean}: {}",
                valuation.clean_price
            );
            assert_eq!(valuation.yield_to_worst, by_price.valuation.yield_to_worst);
            assert_eq!(by_yield.worst.date, by_price.worst.date, "{clean}");
            assert!((by_yield.worst.decimal_yield - valuation.yield_to_worst).abs() < 1e-9);
            assert_eq!(by_yield.workouts.len(), by_price.workouts.len());
            for (from_yield, from_price) in by_yield.workouts.iter().zip(&by_price.workouts) {
                assert_eq!(from_yield.date, from_price.date);
                assert_eq!(from_yield.redemption, from_price.redemption);
                assert!(
                    (from_yield.decimal_yield - from_price.decimal_yield).abs() < 1e-9,
                    "{clean}: {from_yield:?} against {from_price:?}"
                );
            }
            // The valuation is the one Live and request-time quotes give.
            let quoted = value(&bond, SETTLEMENT, WorkoutQuote::YieldToWorst(level)).unwrap();
            assert_eq!(valuation.clean_price, quoted.clean_price);
            assert_eq!(valuation.years_to_worst, quoted.years_to_worst);
        }
        let worst = |clean: &str| {
            calculated(&bond, SETTLEMENT, WorkoutQuote::CleanPrice(decimal(clean)))
                .worst
                .redemption
        };
        assert_eq!(worst("95"), Redemption::Maturity);
        assert_eq!(
            worst("100.25"),
            Redemption::Call {
                call_price: decimal("100")
            }
        );
        assert_eq!(
            worst("104"),
            Redemption::Call {
                call_price: decimal("101")
            }
        );
    }

    #[test]
    fn a_table_with_a_yield_beyond_the_solver_domain_is_refused() {
        // At a 40% yield to worst the price to worst is near 40, at which the January
        // 2027 call at 101 yields more than 500%: Live values the bond from that yield,
        // and the calculator's table cannot be solved.
        let bond = callable(CallStyle::Bermudan);
        let distressed = WorkoutQuote::YieldToWorst(decimal("0.4"));
        let live = value(&bond, SETTLEMENT, distressed).unwrap();
        assert!(live.clean_price < decimal("40"), "{}", live.clean_price);
        let refused = calculate(
            &bond,
            &fixture_calendar(&bond),
            date(SETTLEMENT),
            distressed,
        )
        .err()
        .unwrap();
        assert_eq!(refused.code, ErrorCode::SolverLimit, "{}", refused.message);
        // At 30% every workout's yield is inside it.
        let table = calculated(
            &bond,
            SETTLEMENT,
            WorkoutQuote::YieldToWorst(decimal("0.3")),
        );
        assert!(
            table.workouts[0].decimal_yield > 4.0,
            "{:?}",
            table.workouts[0]
        );
        assert_eq!(table.worst.redemption, Redemption::Maturity);
    }

    #[test]
    fn a_callable_is_quoted_by_a_price_or_a_yield_to_worst_in_range() {
        assert!(matches!(
            WorkoutQuote::quoted_by(&PricingMethod::Price {}, decimal("99")),
            Ok(WorkoutQuote::CleanPrice(_))
        ));
        assert!(matches!(
            WorkoutQuote::quoted_by(&PricingMethod::Yield {}, decimal("0.07")),
            Ok(WorkoutQuote::YieldToWorst(_))
        ));
        assert!(matches!(
            WorkoutQuote::quoted_by(&PricingMethod::Yield {}, decimal("1.5")),
            Err(LevelError::Refused(_))
        ));
        let Err(LevelError::Refused(spread)) =
            WorkoutQuote::quoted_by(&PricingMethod::ZSpread {}, decimal("150"))
        else {
            panic!("a spread does not quote a callable on its workouts")
        };
        assert!(spread.contains("price or a yield to worst"), "{spread}");
    }

    #[test]
    fn a_call_whose_notice_has_passed_is_not_a_workout() {
        let bond = callable(CallStyle::Bermudan);
        // Settling 28 December, the trade is on the 23rd: past the January call's notice
        // deadline, so at a premium the worst is the 2028 call, not the 2027 one.
        let late = value(
            &bond,
            "2026-12-28",
            WorkoutQuote::CleanPrice(decimal("101.25")),
        )
        .unwrap();
        let early = value(
            &bond,
            "2026-12-04",
            WorkoutQuote::CleanPrice(decimal("101.25")),
        )
        .unwrap();
        assert!(late.years_to_worst > 1.0, "{}", late.years_to_worst);
        assert!(early.years_to_worst < 0.2, "{}", early.years_to_worst);
        let dates = |settlement: &str| {
            calculated(
                &bond,
                settlement,
                WorkoutQuote::CleanPrice(decimal("101.25")),
            )
            .workouts
            .iter()
            .map(|workout| workout.date)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            dates("2026-12-28"),
            ["2028-01-15", "2029-01-15", "2030-01-15"].map(date)
        );
        assert_eq!(dates("2026-12-04").len(), 4);
    }

    #[test]
    fn what_the_model_cannot_value_is_refused_with_its_reason() {
        let price = WorkoutQuote::CleanPrice(decimal("101.25"));
        let refused = |bond: &BondReferenceData, settlement: &str| {
            value(bond, settlement, price).err().unwrap()
        };
        let american = refused(&callable(CallStyle::American), SETTLEMENT);
        assert_eq!(american.code, ErrorCode::UnsupportedConvention);
        assert!(
            american.message.contains("European or Bermudan"),
            "{}",
            american.message
        );
        let mut many = callable(CallStyle::Bermudan);
        many.call_schedule = (0..=MAX_WORKOUT_CALLS)
            .map(|week| CallScheduleEntry {
                call_date: date("2027-01-15").add_days(7 * week as i64),
                call_price: decimal("100"),
                is_make_whole: false,
            })
            .collect();
        assert_eq!(
            refused(&many, SETTLEMENT).code,
            ErrorCode::UnsupportedConvention
        );
        many.call_schedule.truncate(MAX_WORKOUT_CALLS);
        assert!(value(&many, SETTLEMENT, price).is_ok());
        // Nothing settles on a Saturday or on Columbus Day.
        let bermudan = callable(CallStyle::Bermudan);
        for closed in ["2026-10-10", "2026-10-12"] {
            let refused = refused(&bermudan, closed);
            assert_eq!(refused.code, ErrorCode::InvalidInput);
            assert!(
                refused.message.contains("no trade settles"),
                "{}",
                refused.message
            );
        }
        // An ACT/ACT ICMA yield needs a reference mesh, which no stored bond carries.
        let mut icma = callable(CallStyle::Bermudan);
        icma.day_count = "ACT/ACT ICMA".into();
        icma.contractual_terms.as_mut().unwrap().settings.day_count =
            convex_core::daycounts::DayCountConvention::ActActIcma;
        assert_eq!(
            refused(&icma, SETTLEMENT).code,
            ErrorCode::UnsupportedConvention
        );
    }
}
