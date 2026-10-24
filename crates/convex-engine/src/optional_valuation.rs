//! Callable valuation through `convex::library::execute`: OAS from the observed clean mid,
//! then effective discount-curve risk at that OAS. Terms, calendars, curves and the model
//! assumption come only from the pinned snapshot; nothing is defaulted or looked up.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::ports::{
    reference_data::{BondReferenceData, BondType},
    reference_master::{CalendarDefinition, CallStyle},
    storage::BondPricingConfig,
};
use crate::pricing_router::PricingRouter;
use convex::library::{
    execute, BondCalendarPins, BondInput, CalendarInput, LibraryError, LibraryErrorCode,
    LibraryInputs, LibraryOperation, LibraryRequest, OptionalInput, OptionalModelInput,
    OptionalOperation, LIBRARY_SCHEMA_VERSION,
};
use convex_analytics::optional_bond as o;
use convex_bonds::{
    instruments::CallableBond,
    types::{CallEntry, CallSchedule, CallType},
};
use convex_core::{
    calendars::Calendar,
    cashflows::PaymentDateEntitlement,
    contracts::{ErrorCode, ReferenceDataId, UtcTimestamp, Version},
    fixings::{FixingContext, UnpublishedFixingPolicy},
    types::{Mark, PriceKind},
    Date,
};
use convex_curves::contracts::{CurveKey, CurveRecipe};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::Value;
use std::collections::BTreeSet;

/// Quantitative model for callables: HW1F trees under the configuration's explicit assumption.
pub const OPTIONAL_SNAPSHOT_MODEL: &str = "convex-optional-hw1f-v1";
/// Measures a callable publishes, each from the library at the solved OAS.
pub const OPTIONAL_MEASURES: [&str; 7] = [
    "clean_price_mid",
    "dirty_price_mid",
    "accrued_interest",
    "oas_mid",
    "effective_duration_mid",
    "effective_convexity_mid",
    "pv01_mid",
];
/// Callables per runtime plan. Each one reprices on HW1F trees on every generation, about
/// 0.8 s in a release build for a ten-year Bermudan (OAS solve 0.46 s, risk 0.21 s).
pub const MAX_RUNTIME_CALLABLES: usize = 8;

// The compiled numerical policy of this model: the repository's validated optional-bond
// settings (`convex/examples/optional_bonds.rs`, `convex-analytics/tests/optional_bonds.rs`).
const SETTINGS: o::OptionalBondSettings = o::OptionalBondSettings {
    grid: o::GridSettings {
        base_steps: 128,
        refinements: 3,
        absolute_price_tolerance: 0.004,
        relative_price_tolerance: 1e-8,
    },
    exercise_priority: o::ExercisePriority::IssuerLast,
};
const OAS_SOLVE: o::OasSolveSettings = o::OasSolveSettings {
    lower: -0.05,
    upper: 0.10,
    price_tolerance: 1e-9,
    max_iterations: 128,
};
const RISK: o::RiskSettings = o::RiskSettings {
    absolute_duration_tolerance: 0.02,
    relative_duration_tolerance: 0.01,
    absolute_convexity_tolerance: 0.25,
    relative_convexity_tolerance: 0.01,
};
/// ±100 bp parallel discount shift; a 1 bp shift is below the tree's rate resolution.
const RISK_BUMP: f64 = 0.01;
/// Holiday tables cover the bond's life plus this margin on each side.
const CALENDAR_MARGIN_DAYS: i64 = 31;

/// Every model a runtime plan's instruments are valued under.
pub const PRODUCT_MODELS: [&str; 3] = [
    crate::snapshot::BOND_SNAPSHOT_MODEL,
    OPTIONAL_SNAPSHOT_MODEL,
    crate::floating_valuation::FLOATING_SNAPSHOT_MODEL,
];
/// The model a pinned bond is valued under.
pub fn product_model(bond: &BondReferenceData) -> &'static str {
    match bond.bond_type {
        BondType::FixedCallable => OPTIONAL_SNAPSHOT_MODEL,
        BondType::FloatingRate => crate::floating_valuation::FLOATING_SNAPSHOT_MODEL,
        _ => crate::snapshot::BOND_SNAPSHOT_MODEL,
    }
}
/// The measures the model publishes; a plan's other measures do not apply to it.
pub fn product_measures(model: &str) -> &'static [&'static str] {
    match model {
        OPTIONAL_SNAPSHOT_MODEL => &OPTIONAL_MEASURES,
        crate::floating_valuation::FLOATING_SNAPSHOT_MODEL => {
            &crate::floating_valuation::FLOATING_MEASURES
        }
        _ => &crate::snapshot::RUNTIME_MEASURES,
    }
}

/// Every generation reprices each callable on HW1F trees, so their number stays bounded.
pub fn within_callable_bound(snapshot: &convex_ports::snapshot::MarketSnapshot) -> bool {
    within_bound(
        snapshot
            .content()
            .references
            .iter()
            .filter_map(|r| r.record.bond.as_ref()),
    )
}
fn within_bound<'a>(bonds: impl Iterator<Item = &'a BondReferenceData>) -> bool {
    bonds
        .filter(|bond| product_model(bond) == OPTIONAL_SNAPSHOT_MODEL)
        .count()
        <= MAX_RUNTIME_CALLABLES
}

/// A failed step, with the canonical code and the library's own reason.
#[derive(Debug, Clone)]
pub(crate) struct Failure {
    pub code: ErrorCode,
    pub message: String,
}
pub(crate) fn failure(code: ErrorCode, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into().chars().take(512).collect(),
    }
}
pub(crate) fn library_failure(error: LibraryError) -> Failure {
    let code = match error.code {
        LibraryErrorCode::MissingInput => ErrorCode::MissingMarketData,
        LibraryErrorCode::NotApplicable | LibraryErrorCode::Unsupported => {
            ErrorCode::UnsupportedConvention
        }
        LibraryErrorCode::NoRoot
        | LibraryErrorCode::NonUnique
        | LibraryErrorCode::NoConvergence
        | LibraryErrorCode::ResourceLimit => ErrorCode::SolverLimit,
        _ => ErrorCode::InvalidInput,
    };
    failure(code, error.message)
}

/// Effective risk under a frozen OAS, or the reason it is unavailable.
pub(crate) struct CallableRisk {
    /// Years, from the ±100 bp discount shift.
    pub duration: f64,
    /// Years squared, from the same shift.
    pub convexity: f64,
    /// Loss per 100 face for +1 bp, scaled from the central difference.
    pub pv01: f64,
}
/// One callable at its observed clean mid.
pub(crate) struct CallableQuote {
    /// Accrued per 100 face from the bond's own schedule.
    pub accrued: f64,
    /// Unrounded basis points over the discount curve.
    pub oas_bps: f64,
    /// Risk is refined independently of price and may fail on its own.
    pub risk: Result<CallableRisk, Failure>,
}

/// Value one callable. `discount` is the exact stored recipe key the configuration selects.
#[allow(clippy::too_many_arguments)]
pub(crate) fn value_callable(
    bond: &BondReferenceData,
    calendar: &CalendarDefinition,
    calendar_version: Version,
    recipes: &[CurveRecipe],
    discount: &CurveKey,
    configuration: &BondPricingConfig,
    settlement: Date,
    known_at: UtcTimestamp,
    clean_mid: Decimal,
) -> Result<CallableQuote, Failure> {
    let unsupported = |reason: &str| failure(ErrorCode::UnsupportedConvention, reason);
    let model = configuration
        .option_model
        .as_ref()
        .ok_or_else(|| unsupported("callable valuation needs the configuration's option_model"))?;
    let terms = bond
        .contractual_terms
        .as_ref()
        .ok_or_else(|| unsupported("callable needs complete contractual terms"))?;
    let call = terms
        .call_terms
        .as_ref()
        .ok_or_else(|| unsupported("callable needs explicit call style and notice terms"))?;
    if bond.bond_type != BondType::FixedCallable
        || !bond.is_callable
        || bond.is_putable
        || bond.is_sinkable
        || bond.floating_terms.is_some()
        || bond.inflation_index.is_some()
        || bond.call_schedule.is_empty()
        || bond.call_schedule.iter().any(|entry| entry.is_make_whole)
    {
        return Err(unsupported(
            "only fixed-price calls on a fixed bullet are valued; puts, sinking funds and make-whole calls are not",
        ));
    }
    let invalid = |reason: String| failure(ErrorCode::InvalidInput, reason);
    let router = PricingRouter { bump_bps: 1.0 };
    let base = router
        .to_fixed_rate_bond(bond)
        .map_err(|e| invalid(e.to_string()))?;
    let frequency = router
        .get_frequency(bond)
        .map_err(|e| invalid(e.to_string()))?;
    // Accrued from the same schedule the bullet path uses; the model's must agree.
    let accrued = convex_analytics::pricing::price_from_mark(
        &base,
        settlement,
        &Mark::Price {
            value: clean_mid,
            kind: PriceKind::Clean,
        },
        None,
        frequency,
    )
    .map_err(|e| invalid(e.to_string()))?
    .accrued_per_100;
    let schedule = CallSchedule {
        call_type: match call.style {
            CallStyle::American => CallType::American,
            CallStyle::European => CallType::European,
            CallStyle::Bermudan => CallType::Bermudan,
        },
        entries: bond
            .call_schedule
            .iter()
            .map(|entry| {
                Ok(CallEntry {
                    start_date: entry.call_date,
                    end_date: None,
                    call_price: entry
                        .call_price
                        .to_f64()
                        .ok_or_else(|| invalid("unrepresentable call price".into()))?,
                    notice_days: Some(call.notice_days),
                })
            })
            .collect::<Result<_, Failure>>()?,
        protection_end: None,
        make_whole_spread: None,
    };
    let contract = CallableBond::new(base, schedule)
        .contract_terms()
        .map_err(|e| invalid(e.to_string()))?;
    let pin = |id: &convex_bonds::types::CalendarId| {
        Ok(convex::rates::ReferencePin {
            id: ReferenceDataId::new(id.as_str())
                .map_err(|_| invalid("invalid native calendar identity".into()))?,
            version: calendar_version,
        })
    };
    let ids = &contract.calendars;
    let pins = BondCalendarPins {
        payment: pin(&ids.payment)?,
        reset: pin(&ids.reset)?,
        observation: pin(&ids.observation)?,
        exercise: pin(&ids.exercise)?,
    };
    let start = terms.dated_date.date().add_days(-CALENDAR_MARGIN_DAYS);
    let end = bond.maturity_date.add_days(CALENDAR_MARGIN_DAYS);
    if start < calendar.coverage_from.date() || end > calendar.coverage_until.date() {
        return Err(unsupported(
            "the pinned calendar's coverage does not span the bond's life",
        ));
    }
    let mut calendars = Vec::new();
    let mut seen = BTreeSet::new();
    for (id, pin) in [
        (&ids.payment, &pins.payment),
        (&ids.reset, &pins.reset),
        (&ids.observation, &pins.observation),
        (&ids.exercise, &pins.exercise),
    ] {
        if seen.insert(id.as_str()) {
            let native = id.try_to_calendar().map_err(|e| invalid(e.to_string()))?;
            calendars.push(holiday_table(native.as_ref(), pin.clone(), start, end)?);
        }
    }
    let input = OptionalInput {
        bond: BondInput {
            terms: contract,
            calendars: pins,
        },
        model: OptionalModelInput::Assumed {
            key: CurveKey {
                id: format!("{}.option-model", configuration.config_id),
                version: configuration.version.to_string(),
            },
            parameters: o::Hw1fParameters {
                mean_reversion: model.mean_reversion,
                volatility: model.volatility,
            },
            reason: model.reason.clone(),
        },
        discount: discount.clone(),
        projection: None,
        projection_context: convex_bonds::contracts::BondProjectionContext {
            settlement,
            on_payment: PaymentDateEntitlement::Exclude,
            fixing: FixingContext {
                valuation_date: settlement,
                known_at,
                unpublished: UnpublishedFixingPolicy::RequirePublished,
                final_only: true,
            },
            overnight_accrual_cutoffs: vec![],
        },
        context: o::OptionalBondContext {
            settlement,
            on_payment: PaymentDateEntitlement::Exclude,
            exercise_state: o::ExerciseState::AliveUnexercised,
        },
        settings: SETTINGS,
    };
    let inputs = LibraryInputs {
        calendars,
        helper_calendars: vec![],
        curves: recipes.to_vec(),
        calibration: None,
        fixings: vec![],
        publications: vec![],
    };
    let run = |action| {
        execute(&LibraryRequest {
            schema_version: LIBRARY_SCHEMA_VERSION,
            inputs: inputs.clone(),
            operation: LibraryOperation::OptionalBond {
                input: Box::new(input.clone()),
                action,
            },
        })
        .map_err(library_failure)
    };
    let clean = clean_mid
        .to_f64()
        .ok_or_else(|| invalid("unrepresentable clean mid".into()))?;
    let solved = run(OptionalOperation::Oas {
        dirty_price: clean + accrued,
        settings: OAS_SOLVE,
    })?;
    let number = |value: &Value, path: &[&str]| {
        path.iter()
            .try_fold(value, |node, key| node.get(key))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .ok_or_else(|| failure(ErrorCode::InvalidInput, "incomplete library response"))
    };
    let oas_decimal = number(&solved, &["data", "oas_decimal"])?;
    let oas_bps = number(&solved, &["data", "oas_bps"])?;
    if (number(&solved, &["data", "price", "accrued"])? - accrued).abs() > 1e-9 {
        return Err(invalid(
            "the optional model's accrued differs from the bond schedule's".into(),
        ));
    }
    let risk = run(OptionalOperation::Risk {
        oas_decimal,
        axis: o::RiskAxis::Discount,
        bump: RISK_BUMP,
        settings: RISK,
    })
    .and_then(|risk| {
        Ok(CallableRisk {
            duration: number(&risk, &["data", "duration"])?,
            convexity: number(&risk, &["data", "convexity"])?,
            pv01: (number(&risk, &["data", "down", "dirty_price"])?
                - number(&risk, &["data", "up", "dirty_price"])?)
                / (2.0 * RISK_BUMP)
                * 1e-4,
        })
    })
    .map_err(|error| Failure {
        code: if error.code == ErrorCode::SolverLimit {
            ErrorCode::IncompleteRisk
        } else {
            error.code
        },
        ..error
    });
    Ok(CallableQuote {
        accrued,
        oas_bps,
        risk,
    })
}

/// The library takes explicit holiday tables, never a calendar code: list the pinned
/// calendar's non-weekend closures over the window. Its weekends must be Saturday and Sunday.
pub(crate) fn holiday_table(
    calendar: &dyn Calendar,
    pin: convex::rates::ReferencePin,
    start: Date,
    end: Date,
) -> Result<CalendarInput, Failure> {
    let mut holidays = Vec::new();
    let mut date = start;
    while date <= end {
        let weekend = date.weekday().num_days_from_monday() >= 5;
        if weekend && calendar.is_business_day(date) {
            return Err(failure(
                ErrorCode::UnsupportedConvention,
                "calendar weekends are not Saturday and Sunday",
            ));
        }
        if !weekend && !calendar.is_business_day(date) {
            holidays.push(date);
        }
        date = date.add_days(1);
    }
    Ok(CalendarInput {
        pin,
        holidays,
        weekend_days: vec![5, 6],
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ports::{
        reference_data::CallScheduleEntry,
        reference_master::{CallTerms, MasterDefinition},
        storage::OptionModelAssumption,
    };
    use convex_curves::contracts::{build_curve_set, CurveSet};
    use std::str::FromStr;

    fn callable(style: CallStyle) -> BondReferenceData {
        let mut bond: BondReferenceData = serde_json::from_str(include_str!(
            "../../convex-ports/tests/fixtures/reference_bond.json"
        ))
        .unwrap();
        bond.bond_type = BondType::FixedCallable;
        bond.is_callable = true;
        bond.call_schedule = [
            ("2027-01-15", "101"),
            ("2028-01-15", "100.5"),
            ("2029-01-15", "100"),
        ]
        .iter()
        .map(|(date, price)| CallScheduleEntry {
            call_date: Date::parse(date).unwrap(),
            call_price: Decimal::from_str(price).unwrap(),
            is_make_whole: false,
        })
        .collect();
        bond.contractual_terms.as_mut().unwrap().call_terms = Some(CallTerms {
            style,
            notice_days: 30,
        });
        bond
    }
    fn config() -> BondPricingConfig {
        let mut config: BondPricingConfig = serde_json::from_str(include_str!(
            "../../convex-ports/tests/fixtures/pricing_config.json"
        ))
        .unwrap();
        config.option_model = Some(OptionModelAssumption {
            mean_reversion: 0.03,
            volatility: 0.01,
            reason: "fictional fixture assumption".into(),
        });
        config
    }
    fn recipe() -> CurveRecipe {
        serde_json::from_value(serde_json::json!({
            "key":{"id":"USD-FIXTURE","version":"supplied.recipe.v7"},
            "reference_date":"2026-10-02", "currency":"USD", "time_basis":"Act365Fixed",
            "role":{"kind":"Discount","collateral_currency":"USD"},
            "representation":{"ZeroRates":{"compounding":"Continuous"}},
            "interpolation":"Linear", "extrapolation":"Flat", "dependencies":[],
            "calibration":"Direct", "source":{"Nodes":[
                {"pillar":{"Tenor":0.0},"quote":{"unit":"DecimalRate","value":0.035}},
                {"pillar":{"Tenor":50.0},"quote":{"unit":"DecimalRate","value":0.035}}
            ]}
        }))
        .unwrap()
    }
    fn quote(bond: &BondReferenceData, clean: &str) -> Result<CallableQuote, Failure> {
        let resources = crate::reference_terms::resources(bond);
        let MasterDefinition::Calendar(calendar) = &resources[1].definition else {
            panic!("fixture calendar resource")
        };
        let recipe = recipe();
        value_callable(
            bond,
            calendar,
            Version::new(1).unwrap(),
            std::slice::from_ref(&recipe),
            &recipe.key,
            &config(),
            Date::parse("2026-10-06").unwrap(),
            UtcTimestamp::parse("2026-10-02T15:00:00Z").unwrap(),
            Decimal::from_str(clean).unwrap(),
        )
    }

    #[test]
    fn the_library_request_matches_the_native_model_on_named_calendars() {
        let bond = callable(CallStyle::Bermudan);
        let ours = quote(&bond, "101.25").unwrap();
        // The same contract on its own named calendars, curve and model, without the library.
        let router = PricingRouter { bump_bps: 1.0 };
        let mut schedule = CallSchedule::new(CallType::Bermudan);
        for entry in &bond.call_schedule {
            schedule.entries.push(CallEntry {
                start_date: entry.call_date,
                end_date: None,
                call_price: entry.call_price.to_f64().unwrap(),
                notice_days: Some(30),
            });
        }
        let contract = CallableBond::new(router.to_fixed_rate_bond(&bond).unwrap(), schedule)
            .contract_terms()
            .unwrap()
            .build()
            .unwrap();
        let curves = build_curve_set(&[recipe()], &CurveSet::new()).unwrap();
        let model = o::OptionalBondModel::assumed(
            CurveKey {
                id: "CONFIG-RESTORE.option-model".into(),
                version: "1".into(),
            },
            o::Hw1fParameters {
                mean_reversion: 0.03,
                volatility: 0.01,
            },
            "fictional fixture assumption".into(),
        )
        .unwrap();
        let exercise = contract
            .terms()
            .calendars
            .exercise
            .try_to_calendar()
            .unwrap();
        let settlement = Date::parse("2026-10-06").unwrap();
        let prepared = o::prepare_fixed(
            &contract,
            o::OptionalBondContext {
                settlement,
                on_payment: PaymentDateEntitlement::Exclude,
                exercise_state: o::ExerciseState::AliveUnexercised,
            },
            o::OptionalBondInputs {
                discount: curves.curves.get(&recipe().key).unwrap().as_ref(),
                projection: None,
                model: &model,
                exercise_calendar: exercise.as_ref(),
            },
            SETTINGS,
        )
        .unwrap();
        let oas = prepared
            .solve_oas(101.25 + ours.accrued, OAS_SOLVE)
            .unwrap();
        assert!((oas.oas_bps - ours.oas_bps).abs() < 1e-9, "{oas:?}");
        assert!((oas.price.accrued - ours.accrued).abs() < 1e-12);
        let risk = prepared
            .risk(oas.oas_decimal, o::RiskAxis::Discount, RISK_BUMP, RISK)
            .unwrap();
        let ours = ours.risk.unwrap();
        assert!((risk.duration - ours.duration).abs() < 1e-12);
        assert!((risk.convexity - ours.convexity).abs() < 1e-12);
        // Face is 100, so the currency PV01 is already per 100 face.
        assert!(
            (risk.pv01 - ours.pv01).abs() < 1e-9,
            "{} {}",
            risk.pv01,
            ours.pv01
        );
        assert!(ours.duration > 0.0 && ours.pv01 > 0.0);
    }

    #[test]
    fn the_call_terms_are_consumed_and_unsupported_exercise_is_refused() {
        let all = quote(&callable(CallStyle::Bermudan), "101.25").unwrap();
        let mut first = callable(CallStyle::Bermudan);
        first.call_schedule.truncate(1);
        let first = quote(&first, "101.25").unwrap();
        // More exercise dates give the issuer more option value, so less spread at one price.
        assert!(all.oas_bps < first.oas_bps);
        // The native model has no validated continuous-exercise discretization.
        let american = quote(&callable(CallStyle::American), "101.25")
            .err()
            .unwrap();
        assert_eq!(american.code, ErrorCode::UnsupportedConvention);
        assert!(
            american.message.contains("American"),
            "{}",
            american.message
        );
        let mut bond = callable(CallStyle::Bermudan);
        bond.contractual_terms.as_mut().unwrap().call_terms = None;
        let refused = quote(&bond, "101.25").err().unwrap();
        assert_eq!(refused.code, ErrorCode::UnsupportedConvention);
        assert!(
            refused.message.contains("call style"),
            "{}",
            refused.message
        );
        let mut bond = callable(CallStyle::Bermudan);
        bond.call_schedule[1].is_make_whole = true;
        assert_eq!(
            quote(&bond, "101.25").err().unwrap().code,
            ErrorCode::UnsupportedConvention
        );
    }

    #[test]
    fn holiday_tables_reproduce_the_named_calendar() {
        let sifma = convex_bonds::types::CalendarId::sifma()
            .try_to_calendar()
            .unwrap();
        let start = Date::parse("2019-12-01").unwrap();
        let end = Date::parse("2030-02-28").unwrap();
        let pin = convex::rates::ReferencePin {
            id: ReferenceDataId::new("SIFMA").unwrap(),
            version: Version::new(1).unwrap(),
        };
        let table = holiday_table(sifma.as_ref(), pin, start, end).unwrap();
        assert!(table.holidays.len() > 80);
        let mut date = start;
        while date <= end {
            assert_eq!(
                Calendar::is_business_day(&table, date),
                sifma.is_business_day(date),
                "{date}"
            );
            date = date.add_days(1);
        }
    }

    #[test]
    fn a_plan_values_at_most_8_callables() {
        let bullet: BondReferenceData = serde_json::from_str(include_str!(
            "../../convex-ports/tests/fixtures/reference_bond.json"
        ))
        .unwrap();
        let mut bonds = vec![callable(CallStyle::Bermudan); MAX_RUNTIME_CALLABLES];
        bonds.push(bullet);
        assert!(within_bound(bonds.iter()));
        bonds.push(callable(CallStyle::European));
        assert!(!within_bound(bonds.iter()));
    }
}
