//! Supplied-input parity examples. Source economics and reference policies are
//! retained from existing fixtures; no candidate result is a numerical oracle.
#![allow(dead_code)]
#[path = "quote_fixture.rs"]
mod cash;
#[path = "../../../convex-analytics/tests/credit_support/mod.rs"]
mod credit;
#[path = "../../../convex-analytics/tests/support/futures.rs"]
mod future;
#[path = "rate_fixture.rs"]
mod rate;
use convex::library::*;
use convex_analytics::{
    credit_risk as cr, optional_bond as o, quote_risk as q, rate_calibration as r, workflow as w,
};
use convex_bonds::{contracts::*, instruments::FixedRateBond};
use convex_core::{
    cashflows::*,
    contracts::*,
    fixings::*,
    types::{Compounding, Frequency},
    Currency, Date,
};
use convex_curves::{contracts::*, ExtrapolationMethod, InterpolationMethod};
use convex_rates::ReferencePin;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
#[path = "library_fixture/extended.rs"]
mod extended;
pub type Error = Box<dyn std::error::Error>;
#[derive(Clone, Serialize)]
pub struct Case {
    pub id: String,
    pub request: LibraryRequest,
    pub expected_error: Option<LibraryErrorCode>,
}
fn add(
    c: &mut Vec<Case>,
    id: impl Into<String>,
    inputs: LibraryInputs,
    operation: LibraryOperation,
) {
    c.push(Case {
        id: id.into(),
        request: LibraryRequest {
            schema_version: 1,
            inputs,
            operation,
        },
        expected_error: None,
    });
}
pub fn date(s: &str) -> Result<Date, Error> {
    Ok(Date::parse(s)?)
}
pub fn at(s: &str) -> Result<UtcTimestamp, Error> {
    Ok(UtcTimestamp::parse(s)?)
}
pub fn n(s: &str) -> Result<ExactDecimal, Error> {
    Ok(ExactDecimal::parse(s)?)
}
fn pin(id: &str) -> Result<ReferencePin, Error> {
    Ok(ReferencePin {
        id: id.to_string().try_into()?,
        version: Version::new(1)?,
    })
}
pub fn bond(terms: BondTerms) -> Result<BondInput, Error> {
    let p = &terms.calendars;
    let calendars = BondCalendarPins {
        payment: pin(p.payment.as_str())?,
        reset: pin(p.reset.as_str())?,
        observation: pin(p.observation.as_str())?,
        exercise: pin(p.exercise.as_str())?,
    };
    Ok(BondInput { terms, calendars })
}
pub fn calendars(v: &Value, holidays: &[Date]) -> Result<Vec<CalendarInput>, Error> {
    fn collect(v: &Value, out: &mut BTreeMap<String, ReferencePin>) -> Result<(), Error> {
        match v {
            Value::Object(o) => {
                for (name, value) in o {
                    if [
                        "accrual",
                        "payment",
                        "fixing",
                        "reset",
                        "observation",
                        "exercise",
                        "calendar",
                        "premium_calendar",
                        "settlement_calendar",
                        "reset_calendar",
                        "fixing_calendar",
                        "observation_calendar",
                        "delivery_calendar",
                    ]
                    .contains(&name.as_str())
                        && value.get("version").is_some_and(Value::is_u64)
                        && value.get("id").is_some()
                    {
                        let p: ReferencePin = serde_json::from_value(value.clone())?;
                        out.insert(p.id.to_string(), p);
                    }
                    collect(value, out)?;
                }
            }
            Value::Array(a) => {
                for value in a {
                    collect(value, out)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut pins = BTreeMap::new();
    collect(v, &mut pins)?;
    let mut holidays = holidays.to_vec();
    holidays.sort();
    holidays.dedup();
    Ok(pins
        .into_values()
        .map(|pin| CalendarInput {
            pin,
            holidays: holidays.clone(),
            weekend_days: vec![5, 6],
        })
        .collect())
}
fn supplied_resources(
    v: &impl Serialize,
    curves: Vec<CurveRecipe>,
    holidays: &[Date],
) -> Result<LibraryInputs, Error> {
    Ok(LibraryInputs {
        calendars: calendars(&serde_json::to_value(v)?, holidays)?,
        helper_calendars: vec![],
        curves,
        calibration: None,
        fixings: vec![],
        publications: vec![],
    })
}
pub fn projection(settlement: Date) -> Result<BondProjectionContext, Error> {
    Ok(BondProjectionContext {
        settlement,
        on_payment: PaymentDateEntitlement::Exclude,
        fixing: FixingContext {
            valuation_date: settlement,
            known_at: at(&format!("{settlement}T00:00:00Z"))?,
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        },
        overnight_accrual_cutoffs: vec![],
    })
}
fn curve(id: &str, role: CurveRole, root: Date, rate: f64) -> CurveRecipe {
    CurveRecipe {
        key: CurveKey {
            id: id.into(),
            version: "synthetic-1".into(),
        },
        role,
        currency: Currency::USD,
        reference_date: root,
        time_basis: convex_core::daycounts::DayCountConvention::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        },
        interpolation: InterpolationMethod::Linear,
        extrapolation: ExtrapolationMethod::Flat,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Tenor(0.0),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
            CurveNode {
                pillar: CurvePoint::Tenor(50.0),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
        ]),
    }
}
fn bonds(c: &mut Vec<Case>) -> Result<(), Error> {
    let f = cash::Fixture::load()?;
    let b = bond(f.helper.terms.clone())?;
    let root = date("2026-10-07")?;
    let holidays: Vec<Date> = serde_json::from_value(f.data["holidays"].clone())?;
    let d = curve(
        "library.original.discount",
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        root,
        0.035,
    );
    let inputs = supplied_resources(&b, vec![d.clone()], &holidays)?;
    let ctx = projection(f.helper.settlement)?;
    let conv = w::DatedYieldConvention {
        day_count: convex_core::daycounts::DayCountConvention::Act365Fixed,
        compounding: Compounding::SemiAnnual,
    };
    let native = b.terms.clone().build_with_calendars(&BondCalendars {
        payment: &f.calendar,
        reset: &f.calendar,
        observation: &f.calendar,
        exercise: &f.calendar,
    })?;
    let cash = w::PreparedBondCash::fixed(&native, ctx.settlement, ctx.on_payment)?;
    let price = cash.price_yield(0.031, conv)?;
    let curves = build_curve_set(std::slice::from_ref(&d), &CurveSet::new())?.curves;
    let spread = cash.price_spread(
        curves.get(&d.key)?.as_ref(),
        w::BondDiscountRole::Collateral,
        0.001,
    )?;
    let settings = w::BondInverseSettings {
        lower: -0.9,
        upper: 1.0,
        price_tolerance: 1e-9,
        parameter_tolerance: 1e-12,
        max_iterations: 256,
    };
    for (id, action) in [
        ("bond.construct", BondOperation::Construct {}),
        ("bond.requirements", BondOperation::Requirements {}),
        ("bond.cashflows", BondOperation::Cashflows {}),
        (
            "bond.yield",
            BondOperation::Price {
                method: BondMethod::Yield {
                    decimal_rate: 0.031,
                    convention: conv,
                },
            },
        ),
        (
            "bond.spread",
            BondOperation::Price {
                method: BondMethod::DiscountSpread {
                    curve: d.key.clone(),
                    role: BondReferenceRole::Collateral {},
                    decimal_spread: 0.001,
                },
            },
        ),
        (
            "bond.yield_inverse",
            BondOperation::YieldInverse {
                target: w::BondPriceTarget::Clean(price.clean_per100),
                convention: conv,
                settings,
            },
        ),
        (
            "bond.spread_inverse",
            BondOperation::SpreadInverse {
                target: w::BondPriceTarget::Dirty(spread.dirty_per100),
                curve: d.key.clone(),
                role: BondReferenceRole::Collateral {},
                settings,
            },
        ),
    ] {
        add(
            c,
            id,
            inputs.clone(),
            LibraryOperation::Bond {
                bond: Box::new(b.clone()),
                context: ctx.clone(),
                projection: None,
                action,
            },
        );
    }
    add(
        c,
        "schedule.independent",
        inputs.clone(),
        LibraryOperation::Schedule {
            terms: b.terms.schedule.clone(),
            dates: b.terms.dates.clone(),
            accrual: b.calendars.payment.clone(),
            payment: b.calendars.payment.clone(),
            fixing: b.calendars.reset.clone(),
            observation: b.calendars.observation.clone(),
            ex_coupon: b.calendars.exercise.clone(),
        },
    );
    add(
        c,
        "helper.cash_quote",
        inputs.clone(),
        LibraryOperation::Helper {
            input: Box::new(HelperInput::CashQuote {
                helper: f.helper,
                calendars: b.calendars,
                reference_date: root,
            }),
        },
    );
    add(
        c,
        "curves.build",
        inputs.clone(),
        LibraryOperation::Curves {},
    );
    for (id, query) in [
        (
            "raw",
            CurveQuery::Raw {
                point: CurvePoint::Tenor(1.0),
            },
        ),
        (
            "df",
            CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(1.0),
            },
        ),
        (
            "ratio",
            CurveQuery::DiscountRatio {
                start: CurvePoint::Tenor(0.5),
                end: CurvePoint::Tenor(1.0),
            },
        ),
        (
            "zero",
            CurveQuery::ZeroRate {
                point: CurvePoint::Tenor(1.0),
                compounding: Compounding::Annual,
                day_count: convex_core::daycounts::DayCountConvention::Act365Fixed,
            },
        ),
        (
            "forward",
            CurveQuery::ForwardRate {
                start: CurvePoint::Tenor(0.5),
                end: CurvePoint::Tenor(1.0),
                compounding: Compounding::Simple,
                day_count: convex_core::daycounts::DayCountConvention::Act365Fixed,
            },
        ),
    ] {
        add(
            c,
            format!("curve.{id}"),
            inputs.clone(),
            LibraryOperation::CurveQuery {
                curve: d.key.clone(),
                query,
            },
        );
    }
    Ok(())
}
fn rates(c: &mut Vec<Case>) -> Result<(), Error> {
    let f = rate::Fixture::load("nonflat-positive")?;
    let context = f.context()?;
    let holidays: Vec<Date> = serde_json::from_value(f.data["holidays"].clone())?;
    let mut inputs = supplied_resources(&f.recipes, vec![], &holidays)?;
    inputs.fixings = f.observations.clone();
    let mut indices = std::collections::BTreeSet::new();
    fn indices_in(v: &Value, out: &mut std::collections::BTreeSet<IndexId>) -> Result<(), Error> {
        match v {
            Value::Object(o) => {
                for (k, v) in o {
                    if k == "index_id" {
                        if let Some(s) = v.as_str() {
                            out.insert(IndexId::new(s)?);
                        }
                    }
                    indices_in(v, out)?;
                }
            }
            Value::Array(a) => {
                for v in a {
                    indices_in(v, out)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    indices_in(&serde_json::to_value(&f.recipes)?, &mut indices)?;
    for index in indices {
        for days in -100..=900 {
            let d = context
                .cashflows
                .fixings
                .valuation_date
                .checked_add_days(days)?;
            inputs.publications.push(FixingRequirement {
                index_id: index.clone(),
                observation_date: d,
                expected_publication: f.publication(&index, d)?,
            });
        }
    }
    let graph = CalibrationInput {
        rates: f
            .recipes
            .iter()
            .map(|recipe| RateCalibrationInput {
                recipe: recipe.clone(),
                quote_versions: recipe
                    .helpers
                    .iter()
                    .map(|_| Version::new(1))
                    .collect::<Result<_, _>>()
                    .unwrap(),
                futures_calendar_stamps: recipe
                    .helpers
                    .iter()
                    .map(|h| {
                        if matches!(h.terms, r::RateCalibrationTerms::RateFuture(_)) {
                            Some(f.calendar_stamp.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
            })
            .collect(),
        credit: vec![],
        cash: vec![],
        rate_context: Some(CalibrationContext {
            cashflows: context.cashflows,
            as_of: context.as_of,
        }),
        controls: FitNumericalControls::default(),
    };
    let stamp_pin = pin(f.calendar_stamp.id.as_str())?;
    if !inputs.calendars.iter().any(|c| c.pin == stamp_pin) {
        inputs.calendars.push(CalendarInput {
            pin: stamp_pin,
            holidays: holidays.clone(),
            weekend_days: vec![5, 6],
        });
    }
    add(
        c,
        "calibration.mixed_rates",
        inputs.clone(),
        LibraryOperation::Calibration {
            input: Box::new(graph.clone()),
            action: CalibrationOperation::Build {},
        },
    );
    let refs = f.reference_curves()?;
    let mut direct = inputs.clone();
    direct.curves = refs
        .keys()
        .map(|k| Ok(refs.get(k)?.recipe().clone()))
        .collect::<Result<_, CurveContractError>>()?;
    for (ri, recipe) in f.recipes.iter().enumerate() {
        for (hi, helper) in recipe.helpers.iter().enumerate() {
            match &helper.terms {
                r::RateCalibrationTerms::Swap(t) => {
                    for (name, action) in [
                        ("construct", SwapOperation::Construct {}),
                        ("requirements", SwapOperation::Requirements {}),
                        ("cashflows", SwapOperation::Cashflows {}),
                        ("price", SwapOperation::Price {}),
                    ] {
                        add(
                            c,
                            format!("swap.{ri}.{hi}.{name}"),
                            direct.clone(),
                            LibraryOperation::Swap {
                                terms: t.clone(),
                                context: context.cashflows,
                                action,
                            },
                        );
                    }
                    if ri == 1 {
                        let product = RiskProductInput::Swap {
                            terms: t.clone(),
                            context: context.cashflows,
                        };
                        add(
                            c,
                            "calibration.price",
                            inputs.clone(),
                            LibraryOperation::Calibration {
                                input: Box::new(graph.clone()),
                                action: CalibrationOperation::Price {
                                    product: Box::new(product.clone()),
                                },
                            },
                        );
                        let key = cash::key(
                            QuoteType::SwapParRate,
                            q::RiskCoordinate::Quote {
                                curve: recipe.curve.key.clone(),
                                helper_id: helper.id.clone(),
                                quote_version: Version::new(1)?,
                            },
                            helper.quote.unit,
                            q::RiskResponse::RebuildDependents,
                            "1Y",
                        )?;
                        add(
                            c,
                            "calibration.quote_scenario",
                            inputs.clone(),
                            LibraryOperation::Calibration {
                                input: Box::new(graph.clone()),
                                action: CalibrationOperation::Scenario {
                                    key: key.clone(),
                                    delta: 0.0001,
                                },
                            },
                        );
                        add(
                            c,
                            "calibration.product_scenario",
                            inputs.clone(),
                            LibraryOperation::Calibration {
                                input: Box::new(graph.clone()),
                                action: CalibrationOperation::ProductScenario {
                                    product: Box::new(product.clone()),
                                    key: key.clone(),
                                    delta: 0.0001,
                                    position: cash::position(
                                        "1",
                                        "1",
                                        q::RiskQuantityUnit::TradeCopies,
                                    )?,
                                    snapshot_id: cash::snapshot()?,
                                },
                            },
                        );
                        add(
                            c,
                            "calibration.quote_risk",
                            inputs.clone(),
                            LibraryOperation::Calibration {
                                input: Box::new(graph.clone()),
                                action: CalibrationOperation::ProductRisk {
                                    product: Box::new(product),
                                    key,
                                    position: cash::position(
                                        "1",
                                        "1",
                                        q::RiskQuantityUnit::TradeCopies,
                                    )?,
                                    snapshot_id: cash::snapshot()?,
                                    settings: cash::settings(0.0001)?,
                                },
                            },
                        );
                    }
                }
                r::RateCalibrationTerms::BasisSwap { terms, .. } => add(
                    c,
                    "basis.price",
                    direct.clone(),
                    LibraryOperation::BasisSwap {
                        terms: terms.clone(),
                        context: context.cashflows,
                    },
                ),
                _ => {}
            }
        }
    }
    Ok(())
}
fn credits(c: &mut Vec<Case>) -> Result<(), Error> {
    let source = credit::inputs()?;
    let recipe = credit::recipe(&source)?;
    let mut terms = source.template.clone();
    terms.survival_curve = recipe.curve.key.clone();
    let input = CreditRiskInput {
        recipe: recipe.clone(),
        terms: terms.clone(),
        context: terms.trade_context(convex_credit::InitialPaymentState::NotContracted),
        controls: FitNumericalControls::default(),
        risk_model: ModelId::new("supplied.credit.central.v1")?,
    };
    let resources = supplied_resources(
        &input,
        vec![source.discount_recipe],
        &source.calendar_holidays,
    )?;
    for (id, action) in [
        ("credit.baseline", CreditRiskOperation::Baseline {}),
        (
            "credit.recovery_scenario",
            CreditRiskOperation::Scenario {
                request: cr::CreditScenarioRequest {
                    id: "supplied.recovery".into(),
                    output_credit: CurveKey {
                        id: recipe.curve.key.id.clone(),
                        version: "recovery.plus".into(),
                    },
                    shock: cr::CreditShock::Recovery {
                        fraction: FiniteF64::new(0.5)?,
                        credit_response: cr::CreditResponse::HoldSurvival,
                    },
                },
            },
        ),
    ] {
        add(
            c,
            id,
            resources.clone(),
            LibraryOperation::CreditRisk {
                input: Box::new(input.clone()),
                action,
            },
        );
    }
    let bump = cr::CreditRiskBump::RunningQuote {
        helper_id: recipe.helpers[0].id.clone(),
        step_bps: 1.0,
    };
    add(
        c,
        "credit.cs01",
        resources.clone(),
        LibraryOperation::CreditRisk {
            input: Box::new(input.clone()),
            action: CreditRiskOperation::Sensitivity {
                factor: credit::factor(&recipe, &bump)?,
                bump,
                revisions: Box::new(credit::revisions(&recipe, "json.cs01", false)),
            },
        },
    );
    let scenario = cr::ImmediateDefaultScenario {
        event_id: "supplied.immediate.default".into(),
        entity_id: terms.reference.entity_id.clone(),
        seniority: terms.reference.seniority,
        default_date: input.context.step_in_date,
        cash_settlement_date: input.context.step_in_date.checked_add_days(5)?,
        accrual_day_fraction: FiniteF64::new(0.5)?,
        auction_recovery: None,
        auction_reference: None,
    };
    add(
        c,
        "credit.jtd_pending",
        resources.clone(),
        LibraryOperation::CreditRisk {
            input: Box::new(input.clone()),
            action: CreditRiskOperation::JumpToDefault {
                scenario: scenario.clone(),
            },
        },
    );
    let mut valued = scenario;
    valued.auction_recovery = Some(FiniteF64::new(0.4)?);
    valued.auction_reference = Some(convex_credit::ReferencePin {
        id: "supplied.auction".to_string().try_into()?,
        version: Version::new(1)?,
    });
    add(
        c,
        "credit.jtd_valued",
        resources.clone(),
        LibraryOperation::CreditRisk {
            input: Box::new(input.clone()),
            action: CreditRiskOperation::JumpToDefault { scenario: valued },
        },
    );
    let mut market = resources;
    market.calibration = Some(Box::new(CalibrationInput {
        rates: vec![],
        credit: vec![recipe],
        cash: vec![],
        rate_context: None,
        controls: FitNumericalControls::default(),
    }));
    for (id, action) in [
        ("cds.construct", CdsOperation::Construct {}),
        ("cds.price", CdsOperation::Price {}),
        (
            "cds.convert",
            CdsOperation::Convert {
                quote: convex_credit::CdsQuote::RunningSpread { decimal_rate: 0.01 },
                settings: convex_credit::ConversionSettings::default(),
            },
        ),
    ] {
        add(
            c,
            id,
            market.clone(),
            LibraryOperation::Cds {
                terms: Box::new(terms.clone()),
                context: input.context.clone(),
                action,
            },
        );
    }
    for (id, query) in [
        (
            "survival",
            CurveQuery::SurvivalProbability {
                point: CurvePoint::Tenor(1.0),
            },
        ),
        (
            "default",
            CurveQuery::DefaultProbability {
                start: CurvePoint::Tenor(0.5),
                end: CurvePoint::Tenor(1.0),
                conditional: true,
            },
        ),
        (
            "hazard",
            CurveQuery::HazardRate {
                point: CurvePoint::Tenor(1.0),
                side: HazardSide::Right,
            },
        ),
    ] {
        add(
            c,
            format!("curve.{id}"),
            market.clone(),
            LibraryOperation::CurveQuery {
                curve: terms.survival_curve.clone(),
                query,
            },
        );
    }
    Ok(())
}
fn futures(c: &mut Vec<Case>) -> Result<(), Error> {
    let f = future::BondPack::new();
    let members = (0..f.bonds.len())
        .map(|i| {
            Ok(BondFutureMember {
                instrument: f.basket.bonds[i].instrument.clone(),
                stamp: f.bond_stamps[i].clone(),
                bond: bond(f.bonds[i].terms().clone())?,
                clean_price_per100: f.prices[i],
                quote_stamp: f.quote_stamps[i].clone(),
                financing: f.financing[i].clone(),
                own_cash: f.own[i].clone(),
                coupon_carry: f.coupon[i].clone(),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let input = BondFutureInput {
        contract: f.contract.clone(),
        basket: f.basket.clone(),
        calendar_stamp: f.calendar_stamp.clone(),
        settlement: future::d("2026-10-08"),
        delivery: future::d("2026-12-08"),
        market: f.market.clone(),
        members,
        selection: convex_analytics::futures::FuturesCtdMethod::MaximumImpliedRepo,
        context: future::ctx(),
    };
    let mut resources = supplied_resources(&input, vec![f.discount.recipe().clone()], &[])?;
    if !resources
        .calendars
        .iter()
        .any(|c| c.pin.id == f.calendar_stamp.id && c.pin.version == f.calendar_stamp.version)
    {
        resources.calendars.push(CalendarInput {
            pin: ReferencePin {
                id: f.calendar_stamp.id.clone(),
                version: f.calendar_stamp.version,
            },
            holidays: vec![],
            weekend_days: vec![5, 6],
        });
    }
    add(
        c,
        "bond_future.price",
        resources.clone(),
        LibraryOperation::BondFuture {
            input: Box::new(input.clone()),
            action: BondFutureOperation::Price {},
        },
    );
    add(
        c,
        "bond_future.risk",
        resources.clone(),
        LibraryOperation::BondFuture {
            input: Box::new(input),
            action: BondFutureOperation::Risk {
                axis: BondFutureAxis::RepoRates {},
                contracts: 2,
                bump: n("0.0001")?,
            },
        },
    );
    add(
        c,
        "future.quote_risk",
        resources.clone(),
        LibraryOperation::FuturesMath {
            action: Box::new(FuturesMathOperation::QuoteRisk {
                contract: f.contract.clone(),
                on: future::d("2026-10-07"),
                contracts: -3,
            }),
        },
    );
    add(
        c,
        "future.round_price",
        resources.clone(),
        LibraryOperation::FuturesMath {
            action: Box::new(FuturesMathOperation::RoundPrice {
                contract: f.contract.clone(),
                on: future::d("2026-10-07"),
                price: n("100.003")?,
                rounding: convex_analytics::futures::FuturesRounding::NearestAwayFromZero,
            }),
        },
    );
    add(
        c,
        "future.round_hedge",
        resources.clone(),
        LibraryOperation::RoundHedge {
            contract: Box::new(f.contract.clone()),
            target_currency_per_bp: n("120")?,
            per_contract_currency_per_bp: n("-50")?,
            rounding: convex_analytics::futures::FuturesRounding::NearestAwayFromZero,
        },
    );
    add(
        c,
        "future.settlement",
        resources,
        LibraryOperation::FuturesMath {
            action: Box::new(FuturesMathOperation::Settlement {
                contract: f.contract.clone(),
                rules: future::rules(&f.contract, None),
                calendar_stamp: f.calendar_stamp.clone(),
                previous: future::mark(
                    &f.contract,
                    "100",
                    "2026-10-07",
                    convex_analytics::futures::FuturesMarkKind::Trade,
                ),
                current: future::mark(
                    &f.contract,
                    "100.125",
                    "2026-10-07",
                    convex_analytics::futures::FuturesMarkKind::DailySettlement,
                ),
                contracts: -3,
                context: future::ctx(),
            }),
        },
    );
    let data = future::fixture();
    let case = data["rates"]["cases"]
        .as_array()
        .ok_or("missing rate cases")?
        .iter()
        .find(|c| c["coverage"] == "mixed")
        .or_else(|| data["rates"]["cases"].as_array()?.first())
        .ok_or("missing rate case")?;
    let f = future::RatePack::new(case);
    let input = RateFutureInput {
        contract: f.contract.clone(),
        calendar_stamp: f.calendar_stamp.clone(),
        clearing: f.clearing.clone(),
        fixing_stamp: f.fixing_stamp.clone(),
        fixing_selection: convex_analytics::futures::FuturesFixingSelection::FirstPublished,
        projection: Some(StampedCurve {
            curve: f.projection.recipe().key.clone(),
            stamp: f.projection_stamp.clone(),
        }),
        convexity_model: None,
        context: f.context,
        unpublished: UnpublishedFixingPolicy::Forecast,
        final_only: true,
    };
    let holidays: Vec<Date> = serde_json::from_value(data["rates"]["holidays"].clone())?;
    let mut resources = supplied_resources(&input, vec![f.projection.recipe().clone()], &holidays)?;
    if !resources
        .calendars
        .iter()
        .any(|c| c.pin.id == f.calendar_stamp.id && c.pin.version == f.calendar_stamp.version)
    {
        resources.calendars.push(CalendarInput {
            pin: ReferencePin {
                id: f.calendar_stamp.id.clone(),
                version: f.calendar_stamp.version,
            },
            holidays,
            weekend_days: vec![5, 6],
        });
    }
    resources.fixings = f.observations.clone();
    let requirements =
        convex_analytics::futures::rate_futures_requirements(&f.inputs(), |i, d| {
            f.publication(i, d)
        })?;
    resources.publications = requirements;
    for (id, action) in [
        (
            "rate_future.requirements",
            RateFutureOperation::Requirements {},
        ),
        ("rate_future.price", RateFutureOperation::Price {}),
        (
            "rate_future.risk",
            RateFutureOperation::Risk {
                contracts: -2,
                bump: n("0.0001")?,
            },
        ),
        (
            "rate_future.remaining",
            RateFutureOperation::RemainingRate { price: n("96")? },
        ),
    ] {
        add(
            c,
            id,
            resources.clone(),
            LibraryOperation::RateFuture {
                input: Box::new(input.clone()),
                action,
            },
        );
    }
    Ok(())
}
fn optional(c: &mut Vec<Case>) -> Result<(), Error> {
    let root = date("2025-01-01")?;
    let maturity = date("2027-01-01")?;
    let mut terms = FixedRateBond::builder()
        .cusip_unchecked("CVX063-synthetic")
        .issue_date(root)
        .maturity(maturity)
        .coupon_rate(Decimal::ZERO)
        .frequency(Frequency::Annual)
        .day_count(convex_core::daycounts::DayCountConvention::Act365Fixed)
        .calendar(convex_bonds::types::CalendarId::weekend_only())
        .business_day_convention(convex_core::calendars::BusinessDayConvention::Unadjusted)
        .end_of_month(false)
        .build()?
        .contract_terms()?;
    terms.exercise.push(ExerciseTerms {
        right: ExerciseRight::Call,
        style: ExerciseStyle::European {
            date: date("2026-01-01")?,
        },
        price_per_100: Decimal::from(96),
        notice_offset: DateOffset::CalendarDays(0),
        settlement_lag: DateOffset::CalendarDays(0),
        settlement_adjustment: convex_core::calendars::BusinessDayConvention::Unadjusted,
        accrued: ExerciseAccrued::PayAccrued,
        protection_end: None,
    });
    let bond = bond(terms)?;
    let d = curve(
        "library.optional.discount",
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        root,
        0.04,
    );
    let input = OptionalInput {
        bond,
        model: OptionalModelInput::Assumed {
            key: CurveKey {
                id: "library.actual.assumed.hw1f".into(),
                version: "1".into(),
            },
            parameters: convex_bonds::options::hw1f::Hw1fParameters {
                mean_reversion: 0.03,
                volatility: 0.02,
            },
            reason: "Explicit synthetic model; existing independent European callable-zero control"
                .into(),
        },
        discount: d.key.clone(),
        projection: None,
        projection_context: projection(root)?,
        context: o::OptionalBondContext {
            settlement: root,
            on_payment: PaymentDateEntitlement::Exclude,
            exercise_state: o::ExerciseState::AliveUnexercised,
        },
        settings: o::OptionalBondSettings {
            grid: o::GridSettings {
                base_steps: 128,
                refinements: 3,
                absolute_price_tolerance: 0.004,
                relative_price_tolerance: 1e-8,
            },
            exercise_priority: o::ExercisePriority::IssuerLast,
        },
    };
    let resources = supplied_resources(&input, vec![d], &[])?;
    for (id, action) in [
        (
            "optional.price",
            OptionalOperation::Price {
                oas_decimal: 0.00123456,
            },
        ),
        (
            "optional.scenario",
            OptionalOperation::Scenario {
                oas_decimal: 0.00123456,
                shift: o::CurveShift {
                    discount: 0.001,
                    projection: 0.0,
                },
            },
        ),
        (
            "optional.oas",
            OptionalOperation::Oas {
                dirty_price: 91.3895997757468,
                settings: o::OasSolveSettings {
                    lower: -0.01,
                    upper: 0.01,
                    price_tolerance: 0.004,
                    max_iterations: 64,
                },
            },
        ),
        (
            "optional.risk",
            OptionalOperation::Risk {
                oas_decimal: 0.00123456,
                axis: o::RiskAxis::Oas,
                bump: 0.01,
                settings: o::RiskSettings {
                    absolute_duration_tolerance: 0.005,
                    relative_duration_tolerance: 0.0,
                    absolute_convexity_tolerance: 0.5,
                    relative_convexity_tolerance: 0.0,
                },
            },
        ),
    ] {
        add(
            c,
            id,
            resources.clone(),
            LibraryOperation::OptionalBond {
                input: Box::new(input.clone()),
                action,
            },
        );
    }
    let mut unsupported = input.clone();
    unsupported.bond.terms.exercise[0].style = ExerciseStyle::American {
        start: date("2026-01-01")?,
        end: maturity,
    };
    add(
        c,
        "optional.unsupported_american",
        resources,
        LibraryOperation::OptionalBond {
            input: Box::new(unsupported),
            action: OptionalOperation::Price { oas_decimal: 0.0 },
        },
    );
    c.last_mut().ok_or("missing case")?.expected_error = Some(LibraryErrorCode::Unsupported);
    Ok(())
}
fn lifecycle_zero(c: &mut Vec<Case>) -> Result<(), Error> {
    let z = ZeroInput {
        identifiers: ZeroIdentifiers {
            cusip: None,
            isin: None,
            figi: None,
            sedol: None,
            ticker: Some("explicit.synthetic.zero".into()),
            issuer_name: None,
            issue_name: None,
        },
        issue_date: date("2025-01-01")?,
        maturity: date("2027-01-01")?,
        issue_price: Some(n("94.12345678901234567890123456")?),
        day_count: convex_core::daycounts::DayCountConvention::Act365Fixed,
        compounding: ZeroCompounding::SemiAnnual,
        settlement_days: 1,
        calendar: pin("weekend.synthetic")?,
        currency: Currency::USD,
        face_value: n("1000")?,
        redemption_value: n("1050")?,
    };
    let resources = supplied_resources(&z, vec![], &[])?;
    for (id, action) in [
        ("zero.construct", ZeroOperation::Construct {}),
        (
            "zero.price",
            ZeroOperation::QuotePrice {
                settlement: z.issue_date,
                clean_price_per100: n("97")?,
            },
        ),
        (
            "zero.yield",
            ZeroOperation::QuoteYield {
                settlement: z.issue_date,
                decimal_yield: n("0.031")?,
                frequency: Frequency::SemiAnnual,
            },
        ),
    ] {
        add(
            c,
            id,
            resources.clone(),
            LibraryOperation::Zero {
                input: Box::new(z.clone()),
                action,
            },
        );
    }
    let mut bill = z.clone();
    bill.maturity = date("2025-04-01")?;
    bill.redemption_value = bill.face_value;
    add(
        c,
        "zero.bill_conversions",
        resources.clone(),
        LibraryOperation::Zero {
            input: Box::new(bill),
            action: ZeroOperation::BillConversions {
                settlement: z.issue_date,
                price_original_face: n("970")?,
            },
        },
    );
    let input = LifecycleInput {
        trade_id: TradeId::new("library.zero.trade")?,
        version: Version::new(1)?,
        instrument: LifecycleInstrumentInput::Zero {
            input: z.clone(),
            credit: None,
        },
        position: convex_analytics::lifecycle::LifecyclePosition::BondFace(n("-3000")?),
        known_at: at("2025-01-01T00:00:00Z")?,
        history: vec![],
        events: vec![LifecycleStep {
            event: convex_analytics::lifecycle::LifecycleEvent {
                event_id: convex_analytics::lifecycle::EventId::new("supplied.maturity")?,
                trade_id: TradeId::new("library.zero.trade")?,
                expected_revision: 0,
                effective_date: z.maturity,
                known_at: at("2027-01-01T12:00:00Z")?,
                action: convex_analytics::lifecycle::LifecycleAction::ContractStep,
            },
            credit_rounding: None,
            futures: None,
            delivery: None,
        }],
    };
    add(
        c,
        "lifecycle.zero",
        resources,
        LibraryOperation::Lifecycle {
            input: Box::new(input),
        },
    );
    Ok(())
}
pub fn cases() -> Result<Vec<Case>, Error> {
    let mut cases = vec![];
    add(
        &mut cases,
        "capabilities",
        LibraryInputs::empty(),
        LibraryOperation::Capabilities {},
    );
    add(
        &mut cases,
        "roundtrip",
        LibraryInputs::empty(),
        LibraryOperation::Roundtrip {},
    );
    bonds(&mut cases)?;
    rates(&mut cases)?;
    credits(&mut cases)?;
    futures(&mut cases)?;
    optional(&mut cases)?;
    lifecycle_zero(&mut cases)?;
    extended::cases(&mut cases)?;
    Ok(cases)
}
