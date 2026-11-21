//! Callable valuation through `convex::library::execute`: the OAS at a clean price, or the
//! price at an OAS, then effective discount-curve risk at that OAS. Terms, calendars,
//! curves and the model assumption come only from the pinned snapshot; nothing is
//! defaulted or looked up.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::level_pricing::{check_level, LevelError};
use crate::ports::{
    level_pricing::PricingMethod,
    reference_data::{BondReferenceData, BondType},
    reference_master::{BondContractTerms, CalendarDefinition, CallStyle, CallTerms},
    storage::BondPricingConfig,
};
use crate::pricing_router::PricingRouter;
use crate::workout_valuation::{WORKOUT_MEASURES, WORKOUT_SNAPSHOT_MODEL};
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
    Date,
};
use convex_curves::contracts::{CurveKey, CurveRecipe};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::Value;
use std::collections::BTreeSet;

/// Quantitative model for callables: HW1F trees under the configuration's explicit assumption.
pub const OPTIONAL_SNAPSHOT_MODEL: &str = "convex-optional-hw1f-v1";
/// Measures a callable publishes, each from the library at its OAS.
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

/// Whether the runtime, not the router, values `bond` at request time: a callable or a
/// floating rate note, whatever its rule. A rule chooses only among runtime models.
pub fn is_runtime_product(bond: &BondReferenceData) -> bool {
    matches!(
        bond.bond_type,
        BondType::FixedCallable | BondType::FloatingRate
    )
}
/// Every model a runtime plan's instruments are valued under.
pub const PRODUCT_MODELS: [&str; 4] = [
    crate::snapshot::BOND_SNAPSHOT_MODEL,
    OPTIONAL_SNAPSHOT_MODEL,
    WORKOUT_SNAPSHOT_MODEL,
    crate::floating_valuation::FLOATING_SNAPSHOT_MODEL,
];
/// The model a pinned bond is valued under, quoted by `method` when it has a pricing rule:
/// a callable quoted by price or yield to worst on its workouts, any other callable on
/// HW1F trees.
pub fn product_model(bond: &BondReferenceData, method: Option<&PricingMethod>) -> &'static str {
    match bond.bond_type {
        BondType::FixedCallable
            if matches!(
                method,
                Some(PricingMethod::Price {} | PricingMethod::Yield {})
            ) =>
        {
            WORKOUT_SNAPSHOT_MODEL
        }
        BondType::FixedCallable => OPTIONAL_SNAPSHOT_MODEL,
        BondType::FloatingRate => crate::floating_valuation::FLOATING_SNAPSHOT_MODEL,
        _ => crate::snapshot::BOND_SNAPSHOT_MODEL,
    }
}
/// Whether `model` values a bond's level itself, as well as a price, so the level stage
/// does not price it first: a callable's yield to worst on its workouts or its OAS on
/// HW1F trees, and a floating rate note's discount margin.
pub fn values_own_level(model: &str) -> bool {
    model == WORKOUT_SNAPSHOT_MODEL
        || model == OPTIONAL_SNAPSHOT_MODEL
        || model == crate::floating_valuation::FLOATING_SNAPSHOT_MODEL
}
/// The measures the model publishes; a plan's other measures do not apply to it.
pub fn product_measures(model: &str) -> &'static [&'static str] {
    match model {
        OPTIONAL_SNAPSHOT_MODEL => &OPTIONAL_MEASURES,
        WORKOUT_SNAPSHOT_MODEL => &WORKOUT_MEASURES,
        crate::floating_valuation::FLOATING_SNAPSHOT_MODEL => {
            &crate::floating_valuation::FLOATING_MEASURES
        }
        _ => &crate::snapshot::RUNTIME_MEASURES,
    }
}

/// Every generation reprices each callable on HW1F trees, so their number stays bounded.
/// Callables on their workouts are not counted.
pub fn within_callable_bound(snapshot: &convex_ports::snapshot::MarketSnapshot) -> bool {
    references_within_callable_bound(&snapshot.content().references)
}
/// [`within_callable_bound`] for one reference section, which is all it reads.
pub(crate) fn references_within_callable_bound(
    references: &convex_ports::snapshot::SnapshotReferences,
) -> bool {
    within_bound(
        references
            .iter()
            .filter_map(|r| Some((r.record.bond.as_ref()?, r.pricing_method()))),
    )
}
fn within_bound<'a>(
    bonds: impl Iterator<Item = (&'a BondReferenceData, Option<&'a PricingMethod>)>,
) -> bool {
    bonds
        .filter(|(bond, method)| product_model(bond, *method) == OPTIONAL_SNAPSHOT_MODEL)
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
/// `value` as a float, refused as invalid input when it has none.
pub(crate) fn to_f64(value: Decimal, what: &str) -> Result<f64, Failure> {
    value
        .to_f64()
        .ok_or_else(|| failure(ErrorCode::InvalidInput, format!("unrepresentable {what}")))
}
/// A level refused for a measure, with its code and reason.
pub(crate) fn level_failure(error: crate::level_pricing::LevelError) -> Failure {
    failure(error.error_code(), error.to_string())
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
/// What a callable on HW1F trees is valued from.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CallableQuote {
    /// A clean price per 100 face.
    CleanPrice(Decimal),
    /// An OAS over the discount curve, in basis points.
    OasBps(Decimal),
}

impl CallableQuote {
    /// `level` as `method` quotes a callable on HW1F trees: a clean price, or an OAS
    /// within the range a rule's OAS takes.
    pub(crate) fn quoted_by(method: &PricingMethod, level: Decimal) -> Result<Self, LevelError> {
        match method {
            PricingMethod::Price {} => Ok(Self::CleanPrice(level)),
            PricingMethod::Oas {} => {
                check_level(method, level)?;
                Ok(Self::OasBps(level))
            }
            PricingMethod::Yield {}
            | PricingMethod::ZSpread {}
            | PricingMethod::BenchmarkSpread { .. }
            | PricingMethod::DiscountMargin {} => Err(LevelError::Refused(format!(
                "a callable is valued on HW1F trees from a price or an OAS, not a {}",
                method.name()
            ))),
        }
    }
}

/// One callable valued at its quote.
pub(crate) struct CallableValuation {
    /// Clean price per 100 face: the quoted price, or the model's at the quoted OAS.
    pub clean: Decimal,
    /// Dirty price per 100 face.
    pub dirty: f64,
    /// Accrued per 100 face from the bond's own schedule, which the model's agrees with.
    pub accrued: f64,
    /// Unrounded basis points over the discount curve: the solved OAS, or the OAS as
    /// quoted.
    pub oas_bps: f64,
    /// Risk is refined independently of price and may fail on its own.
    pub risk: Result<CallableRisk, Failure>,
}

/// Value one callable at `quote`: from a clean price, the OAS that reprices it; from an
/// OAS, the price at it. `discount` is the exact stored recipe key the configuration
/// selects.
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
    quote: CallableQuote,
) -> Result<CallableValuation, Failure> {
    let unsupported = |reason: &str| failure(ErrorCode::UnsupportedConvention, reason);
    let model = configuration
        .option_model
        .as_ref()
        .ok_or_else(|| unsupported("callable valuation needs the configuration's option_model"))?;
    let (terms, call) = stored_call_terms(bond)?;
    if bond.bond_type != BondType::FixedCallable
        || !bond.is_callable
        || bond.has_terms_beyond_calls()
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
    // Accrued from the same schedule the bullet path uses; the model's must agree.
    let accrued = to_f64(
        base.try_accrued_interest(settlement)
            .map_err(|e| invalid(e.to_string()))?,
        "accrued",
    )?;
    let contract = callable_terms(base, bond, call)?;
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
    let (start, end) = calendar_window(bond, terms, calendar)?;
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
    let number = |value: &Value, path: &[&str]| {
        path.iter()
            .try_fold(value, |node, key| node.get(key))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .ok_or_else(|| failure(ErrorCode::InvalidInput, "incomplete library response"))
    };
    let agrees_with_schedule = |model_accrued: f64| {
        if (model_accrued - accrued).abs() > 1e-9 {
            return Err(invalid(
                "the optional model's accrued differs from the bond schedule's".into(),
            ));
        }
        Ok(())
    };
    let risk_at = |oas_decimal| {
        run(OptionalOperation::Risk {
            oas_decimal,
            axis: o::RiskAxis::Discount,
            bump: RISK_BUMP,
            settings: RISK,
        })
    };
    let callable_risk = |risk: Result<Value, Failure>| {
        risk.and_then(|risk| {
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
        })
    };
    match quote {
        CallableQuote::CleanPrice(clean) => {
            let dirty = to_f64(clean, "clean mid")? + accrued;
            let solved = run(OptionalOperation::Oas {
                dirty_price: dirty,
                settings: OAS_SOLVE,
            })?;
            agrees_with_schedule(number(&solved, &["data", "price", "accrued"])?)?;
            let risk = risk_at(number(&solved, &["data", "oas_decimal"])?);
            Ok(CallableValuation {
                clean,
                dirty,
                accrued,
                oas_bps: number(&solved, &["data", "oas_bps"])?,
                risk: callable_risk(risk),
            })
        }
        CallableQuote::OasBps(oas) => {
            let oas_bps = to_f64(oas, "OAS")?;
            let oas_decimal = oas_bps * 1e-4;
            // Risk reports the price at the OAS it bumps from, so the price is run alone only
            // when risk fails.
            let risk = risk_at(oas_decimal);
            let priced_alone;
            let price = match &risk {
                Ok(risk) => &risk["data"]["base"],
                Err(_) => {
                    priced_alone = run(OptionalOperation::Price { oas_decimal })?;
                    &priced_alone["data"]
                }
            };
            agrees_with_schedule(number(price, &["accrued"])?)?;
            Ok(CallableValuation {
                clean: Decimal::from_f64_retain(number(price, &["clean_price"])?)
                    .ok_or_else(|| invalid("the price at the OAS is not finite".into()))?,
                dirty: number(price, &["dirty_price"])?,
                accrued,
                oas_bps,
                risk: callable_risk(risk),
            })
        }
    }
}

/// A stored callable's contractual terms and its call mechanics, which a callable is valued
/// only with.
pub(crate) fn stored_call_terms(
    bond: &BondReferenceData,
) -> Result<(&BondContractTerms, &CallTerms), Failure> {
    let unsupported = |reason: &str| failure(ErrorCode::UnsupportedConvention, reason);
    let terms = bond
        .contractual_terms
        .as_ref()
        .ok_or_else(|| unsupported("callable needs complete contractual terms"))?;
    let call = terms
        .call_terms
        .as_ref()
        .ok_or_else(|| unsupported("callable needs explicit call style and notice terms"))?;
    Ok((terms, call))
}

/// A stored callable's native contract terms: the router's bullet with its call schedule,
/// each call at its date and price under the bond's call style and notice.
pub(crate) fn callable_terms(
    base: convex_bonds::prelude::FixedRateBond,
    bond: &BondReferenceData,
    call: &CallTerms,
) -> Result<convex_bonds::contracts::BondTerms, Failure> {
    let invalid = |reason: String| failure(ErrorCode::InvalidInput, reason);
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
    CallableBond::new(base, schedule)
        .contract_terms()
        .map_err(|e| invalid(e.to_string()))
}

/// The bond's life plus a margin each side, which the pinned calendar must cover.
pub(crate) fn calendar_window(
    bond: &BondReferenceData,
    terms: &BondContractTerms,
    calendar: &CalendarDefinition,
) -> Result<(Date, Date), Failure> {
    let start = terms.dated_date.date().add_days(-CALENDAR_MARGIN_DAYS);
    let end = bond.maturity_date.add_days(CALENDAR_MARGIN_DAYS);
    if start < calendar.coverage_from.date() || end > calendar.coverage_until.date() {
        return Err(failure(
            ErrorCode::UnsupportedConvention,
            "the pinned calendar's coverage does not span the bond's life",
        ));
    }
    Ok((start, end))
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
    use crate::ports::{reference_master::MasterDefinition, storage::OptionModelAssumption};
    use convex_curves::contracts::{build_curve_set, CurveSet};
    use std::str::FromStr;

    fn callable(style: CallStyle) -> BondReferenceData {
        crate::products::callable(
            serde_json::from_str(include_str!(
                "../../convex-ports/tests/fixtures/reference_bond.json"
            ))
            .unwrap(),
            style,
        )
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
    fn quote(bond: &BondReferenceData, clean: &str) -> Result<CallableValuation, Failure> {
        value(
            bond,
            CallableQuote::CleanPrice(Decimal::from_str(clean).unwrap()),
        )
    }
    fn value(bond: &BondReferenceData, quote: CallableQuote) -> Result<CallableValuation, Failure> {
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
            quote,
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
    fn the_price_at_the_solved_oas_is_the_price() {
        let bond = callable(CallStyle::Bermudan);
        let mut wider = f64::INFINITY;
        for clean in ["97", "99.50", "101.25"] {
            let solved = quote(&bond, clean).unwrap();
            // A higher price is a tighter OAS.
            assert!(solved.oas_bps < wider);
            wider = solved.oas_bps;
            let oas = Decimal::from_f64_retain(solved.oas_bps).unwrap();
            let priced = value(&bond, CallableQuote::OasBps(oas)).unwrap();
            let clean = Decimal::from_str(clean).unwrap();
            assert!(
                (priced.clean - clean).abs() < Decimal::new(1, 6),
                "{}",
                priced.clean
            );
            assert!((priced.dirty - solved.dirty).abs() < 1e-6);
            assert_eq!(priced.accrued, solved.accrued);
            assert_eq!(priced.oas_bps, solved.oas_bps);
            // Risk at one OAS is the same either way, or fails the same way.
            match (solved.risk, priced.risk) {
                (Ok(solved), Ok(priced)) => {
                    assert!((priced.duration - solved.duration).abs() < 1e-9);
                    assert!((priced.pv01 - solved.pv01).abs() < 1e-9);
                }
                (Err(solved), Err(priced)) => assert_eq!(priced.code, solved.code),
                _ => panic!("risk differs at {clean}"),
            }
        }
    }

    #[test]
    fn an_oas_in_range_prices_and_solves_back_and_any_other_level_is_refused() {
        use crate::level_pricing::OAS_LEVEL_RANGE_BPS as RANGE;
        // The range is the OAS solve's bracket.
        let decimal = |bps: &Decimal| bps.to_f64().unwrap() / 1e4;
        assert_eq!(decimal(RANGE.start()), OAS_SOLVE.lower);
        assert_eq!(decimal(RANGE.end()), OAS_SOLVE.upper);
        let bond = callable(CallStyle::Bermudan);
        let oas = PricingMethod::Oas {};
        for end in [*RANGE.start(), *RANGE.end()] {
            let priced = value(&bond, CallableQuote::quoted_by(&oas, end).unwrap()).unwrap();
            let solved = quote(&bond, &priced.clean.to_string()).unwrap();
            assert!(
                (solved.oas_bps - priced.oas_bps).abs() < 1e-3,
                "{end}: {}",
                solved.oas_bps
            );
        }
        for outside in [*RANGE.start() - Decimal::ONE, *RANGE.end() + Decimal::ONE] {
            assert!(
                CallableQuote::quoted_by(&oas, outside).is_err(),
                "{outside}"
            );
        }
        for method in [PricingMethod::Yield {}, PricingMethod::ZSpread {}] {
            assert!(CallableQuote::quoted_by(&method, Decimal::ONE).is_err());
        }
        assert!(matches!(
            CallableQuote::quoted_by(&PricingMethod::Price {}, Decimal::ONE_HUNDRED),
            Ok(CallableQuote::CleanPrice(price)) if price == Decimal::ONE_HUNDRED
        ));
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
    fn a_plan_values_at_most_8_callables_on_trees_and_any_number_on_workouts() {
        let bullet: BondReferenceData = serde_json::from_str(include_str!(
            "../../convex-ports/tests/fixtures/reference_bond.json"
        ))
        .unwrap();
        let on_trees = |bonds: &[BondReferenceData], method: Option<&PricingMethod>| {
            within_bound(bonds.iter().map(|bond| (bond, method)))
        };
        let mut bonds = vec![callable(CallStyle::Bermudan); MAX_RUNTIME_CALLABLES];
        bonds.push(bullet);
        assert!(on_trees(&bonds, None));
        bonds.push(callable(CallStyle::European));
        assert!(!on_trees(&bonds, None));
        // Quoted by price or yield to worst, each is valued on its workouts instead.
        assert!(on_trees(&bonds, Some(&PricingMethod::Price {})));
        assert!(on_trees(&bonds, Some(&PricingMethod::Yield {})));
        assert!(!on_trees(&bonds, Some(&PricingMethod::ZSpread {})));
    }
}
