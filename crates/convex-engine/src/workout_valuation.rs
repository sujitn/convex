//! Callables on their workouts: a callable quoted by price or by its yield to worst is
//! valued on each call date and maturity, as high-yield callables are quoted, with no
//! option model and no curve. From a clean price the model solves the yield to worst;
//! from a yield to worst it prices to worst. The yields are dated yields under the bond's
//! own day count, compounded at its coupon frequency, which for a 30/360 or ACT/ACT ISDA
//! bond is its street yield. Every formula is the pure crates'
//! (`convex_analytics::workflow`): the engine builds the contract and its context only.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_analytics::workflow::{
    quote_workout_price_to_worst, quote_workout_worst, BondInverseSettings, BondPriceTarget,
    DatedYieldConvention, WorkflowError, WorkoutContext, WorkoutInputs, WorkoutNotice,
    WorkoutSelection, WorkoutSolveSettings, MAX_WORKOUT_CASH_EVALUATIONS,
};
use convex_core::{
    cashflows::{latest_trade_date, PaymentDateEntitlement},
    contracts::ErrorCode,
    types::Compounding,
    Date,
};
use rust_decimal::{prelude::ToPrimitive, Decimal};

use crate::optional_valuation::{
    calendar_window, callable_terms, failure, library_failure, stored_call_terms, Failure,
};
use crate::ports::{
    level_pricing::{is_workout_callable, MAX_WORKOUT_CALLS},
    reference_data::BondReferenceData,
    reference_master::CalendarDefinition,
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

/// A callable valued on its workouts.
#[derive(Debug, Clone)]
pub(crate) struct WorkoutValuation {
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

/// Value a stored callable on its workouts at `settlement`. The calls considered are the
/// ones whose notice can still be given on or after the trade date that settles then: no
/// notice is stored, so no call is taken as already noticed.
pub(crate) fn value_on_workouts(
    bond: &BondReferenceData,
    calendar: &CalendarDefinition,
    settlement: Date,
    quote: WorkoutQuote,
) -> Result<WorkoutValuation, Failure> {
    let invalid = |reason: String| failure(ErrorCode::InvalidInput, reason);
    if !is_workout_callable(bond) {
        return Err(failure(
            ErrorCode::UnsupportedConvention,
            "a callable is valued on its workouts with European or Bermudan fixed-price calls, at most 128, and no other embedded terms",
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
    // The router builds a stored bond on one calendar for payment, reset, observation and
    // exercise.
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
    let inputs = WorkoutInputs {
        contract: &contract,
        context: &context,
        exercise_calendar: (&source.calendars.exercise, calendar.as_ref()),
        purchase_calendar: (&source.calendars.payment, calendar.as_ref()),
    };
    let convention = DatedYieldConvention {
        day_count: source.day_count,
        compounding: Compounding::from(source.schedule.frequency),
    };
    let number = |value: Decimal, what: &str| {
        value
            .to_f64()
            .ok_or_else(|| invalid(format!("unrepresentable {what}")))
    };
    let years_to = |selection: &WorkoutSelection| {
        let date = match selection {
            WorkoutSelection::Maturity {} => source.schedule.end_date,
            WorkoutSelection::Exercise { date, .. } => *date,
        };
        convex_curves::contracts::year_fraction(settlement, date, source.day_count)
            .map_err(|e| invalid(e.to_string()))
    };
    let solved = |error: WorkflowError| library_failure(error.into());
    match quote {
        WorkoutQuote::CleanPrice(clean) => {
            let target = BondPriceTarget::Clean(number(clean, "clean price")?);
            let worst =
                quote_workout_worst(&inputs, convention, target, WORST_SOLVE).map_err(solved)?;
            let path = &worst.quotes[worst.selected_index];
            Ok(WorkoutValuation {
                clean_price: clean,
                dirty_price: path.dirty_per100,
                accrued: path.accrued_per100,
                yield_to_worst: path.decimal_yield,
                years_to_worst: years_to(&path.selection)?,
            })
        }
        WorkoutQuote::YieldToWorst(level) => {
            let level = number(level, "yield")?;
            let priced =
                quote_workout_price_to_worst(&inputs, convention, level, WORST_SOLVE.max_dates)
                    .map_err(solved)?;
            let path = &priced.quotes[priced.selected_index];
            Ok(WorkoutValuation {
                clean_price: Decimal::from_f64_retain(path.clean_per100)
                    .ok_or_else(|| invalid("the price to worst is not finite".into()))?,
                dirty_price: path.dirty_per100,
                accrued: path.accrued_per100,
                yield_to_worst: level,
                years_to_worst: years_to(&path.selection)?,
            })
        }
    }
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
    fn value(
        bond: &BondReferenceData,
        settlement: &str,
        quote: WorkoutQuote,
    ) -> Result<WorkoutValuation, Failure> {
        let resources = crate::reference_terms::resources(bond);
        let MasterDefinition::Calendar(calendar) = &resources[1].definition else {
            panic!("fixture calendar resource")
        };
        value_on_workouts(bond, calendar, date(settlement), quote)
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
