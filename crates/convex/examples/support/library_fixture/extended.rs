//! Actual complete supplied native cases supplementing the adapter corpus.
use super::*;
use convex_analytics::{cash_calibration as cc, curve_inputs as h, futures as f, lifecycle as l};
use convex_core::{daycounts::DayCountConvention as Dc, futures::*};
#[path = "../cash_fixture.rs"]
mod cf;
#[path = "../lifecycle_fixture.rs"]
mod lf;

pub fn cases(c: &mut Vec<Case>) -> Result<(), Error> {
    cash_curves(c)?;
    helpers(c)?;
    frn_lifecycle(c)?;
    cash_roll(c)?;
    credit_hedge(c)?;
    quoted_risk(c)?;
    model_calibration(c)?;
    negative(c)?;
    Ok(())
}
fn cash_curves(c: &mut Vec<Case>) -> Result<(), Error> {
    for id in ["positive", "negative"] {
        let f = cf::Fixture::load(id)?;
        let calendars = f
            .recipes
            .iter()
            .map(|r| {
                Ok(CashCalibrationInput {
                    recipe: r.clone(),
                    calendars: r
                        .helpers
                        .iter()
                        .map(|h| match h {
                            cc::CashCalibrationHelper::Bond { input } => {
                                Ok(CashHelperCalendars::Bond {
                                    pins: bond(input.terms.clone())?.calendars,
                                })
                            }
                            cc::CashCalibrationHelper::Repo { input } => {
                                Ok(CashHelperCalendars::Repo {
                                    pin: input.calendar.clone(),
                                })
                            }
                        })
                        .collect::<Result<_, Error>>()?,
                })
            })
            .collect::<Result<_, Error>>()?;
        let input = CalibrationInput {
            rates: vec![],
            credit: vec![],
            cash: calendars,
            rate_context: None,
            controls: FitNumericalControls::default(),
        };
        let holidays = serde_json::from_value::<Vec<Date>>(f.row["holidays"].clone())?;
        let resources = supplied_resources(&input, f.direct_funding.clone(), &holidays)?;
        add(
            c,
            format!("calibration.cash.{id}"),
            resources.clone(),
            LibraryOperation::Calibration {
                input: Box::new(input.clone()),
                action: CalibrationOperation::Build {},
            },
        );
        let mut market = resources;
        market.calibration = Some(Box::new(input));
        for (i, t) in f.forwards.iter().enumerate() {
            let forward = RepoForwardInput {
                terms: t.clone(),
                calendars: bond(t.spot.terms.clone())?.calendars,
            };
            add(
                c,
                format!("repo.forward.{id}.{i}"),
                market.clone(),
                LibraryOperation::RepoForward {
                    input: Box::new(forward),
                },
            );
            add(
                c,
                format!("repo.period.{id}.{i}"),
                market.clone(),
                LibraryOperation::Helper {
                    input: Box::new(HelperInput::RepoRate {
                        funding: t.repo.clone(),
                        currency: t.spot.terms.currency,
                        start: t.spot.settlement,
                        end: t.delivery,
                        day_count: Dc::Act360,
                    }),
                },
            );
        }
        for r in &f.recipes {
            if let Some(model) = &r.basis_model {
                if let Some(cc::CashCalibrationHelper::Bond { input: h }) = r.helpers.first() {
                    add(
                        c,
                        format!("basis.bond.{id}"),
                        market.clone(),
                        LibraryOperation::Helper {
                            input: Box::new(HelperInput::BasisBond {
                                helper: h.as_ref().clone(),
                                calendars: bond(h.terms.clone())?.calendars,
                                model: model.clone(),
                                basis: r.curve.key.clone(),
                            }),
                        },
                    );
                }
            }
        }
    }
    Ok(())
}
fn helpers(c: &mut Vec<Case>) -> Result<(), Error> {
    let v: Value = serde_json::from_str(include_str!(
        "../../../../convex-analytics/tests/fixtures/curve-helper-inputs.json"
    ))?;
    let originals: Vec<h::CurveHelperInput> = serde_json::from_value(v["helpers"].clone())?;
    let mut resources = LibraryInputs::empty();
    resources.calendars = vec![CalendarInput {
        pin: pin("WEEKEND")?,
        holidays: vec![],
        weekend_days: vec![5, 6],
    }];
    resources.helper_calendars = vec![HelperCalendarInput {
        pin: h::ReferencePin {
            id: "WEEKEND".into(),
            version: "supplied-v1".into(),
        },
        calendar: pin("WEEKEND")?,
    }];
    for (i, helper) in originals.into_iter().enumerate() {
        add(
            c,
            format!("helper.describe.{i}"),
            resources.clone(),
            LibraryOperation::Helper {
                input: Box::new(HelperInput::Describe {
                    helper,
                    reference_date: date("2024-01-01")?,
                }),
            },
        );
    }
    let end = date("2024-04-01")?;
    let root = date("2024-01-01")?;
    let helper = h::CurveHelperInput {
        id: "deposit".into(),
        currency: Currency::USD,
        terms: h::CurveHelperTerms::MoneyMarket(MoneyMarketTerms::Deposit {
            start_date: root,
            maturity_date: end,
            day_count: Dc::Act360,
        }),
        quote: MarketQuote {
            value: 5.0,
            unit: QuoteUnit::PercentRate,
        },
        pillar: h::HelperPillar::Maturity,
        tolerance: 1e-10,
        weight: 1.0,
        residual_scale: 100.0,
    };
    let mut recipe = curve(
        "library.deposit",
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        root,
        0.03,
    );
    recipe.representation = CurveRepresentation::DiscountFactors;
    recipe.interpolation = InterpolationMethod::LogLinear;
    recipe.extrapolation = ExtrapolationMethod::None;
    recipe.calibration = CalibrationMode::Piecewise;
    recipe.source = CurveSource::Helpers(vec![helper.id.clone()]);
    add(
        c,
        "helper.build.deposit",
        resources,
        LibraryOperation::Helper {
            input: Box::new(HelperInput::Build {
                recipes: vec![h::HelperCurveRecipe {
                    recipe,
                    helpers: vec![helper],
                }],
            }),
        },
    );
    let dates = vec![date("2024-07-01")?, date("2025-01-01")?];
    let mut recipe = curve(
        "library.fit.negative",
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        root,
        0.01,
    );
    recipe.extrapolation = ExtrapolationMethod::None;
    recipe.source = CurveSource::MoneyMarket(
        dates
            .iter()
            .enumerate()
            .map(|(i, end)| {
                MoneyMarketHelper {
                    id: format!("deposit-{i}"),
                    terms: MoneyMarketTerms::Deposit {
                        start_date: root,
                        maturity_date: *end,
                        day_count: Dc::Act360,
                    },
                    // Pinned from tests/fixtures/library/cases.json: deriving them with
                    // exp_m1 differs by one ulp between MSVC and glibc libm.
                    quote: MarketQuote {
                        value: [-0.0196279936343971, -195.2954236536743][i],
                        unit: [QuoteUnit::DecimalRate, QuoteUnit::BasisPoints][i],
                    },
                    tolerance: [1e-10, 1e-6][i],
                }
            })
            .collect(),
    );
    recipe.calibration = CalibrationMode::Global(GlobalFitSettings {
        max_iterations: 100,
        objective_tolerance: 1e-11,
        initial_zeros: vec![0.03, 0.01],
        lower_zero: -0.05,
        upper_zero: 0.1,
        quote_scales: vec![1.0, 10000.0],
        weights: vec![1.0, 2.0],
    });
    add(
        c,
        "curve.fit.dated",
        LibraryInputs::empty(),
        LibraryOperation::CurveFit {
            recipe,
            pillars: dates,
            controls: FitNumericalControls::default(),
        },
    );
    Ok(())
}
fn frn_lifecycle(c: &mut Vec<Case>) -> Result<(), Error> {
    let f = lf::Fixture::load()?;
    let mut terms: BondTerms = serde_json::from_value(f.data["frn"].clone())?;
    terms.exercise.clear();
    let b = bond(terms)?;
    let settlement = date("2024-01-02")?;
    let ctx = projection(settlement)?;
    let p = curve(
        "library.frn.projection",
        CurveRole::Projection {
            index_id: "USD.TERM".into(),
        },
        settlement,
        0.03,
    );
    let d = curve(
        "library.frn.discount",
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        settlement,
        0.025,
    );
    let mut inputs = supplied_resources(&b, vec![p.clone(), d.clone()], &[])?;
    let contract = b.terms.clone().build()?;
    inputs.publications =
        contract.fixing_requirements(ctx.settlement, ctx.on_payment, |_, d| {
            UtcTimestamp::parse(&format!("{d}T08:00:00Z")).map_err(|e| {
                CashflowError::InvalidTerms {
                    field: "synthetic.publication",
                    reason: e.to_string(),
                }
            })
        })?;
    let method = BondMethod::DiscountSpread {
        curve: d.key,
        role: BondReferenceRole::Collateral {},
        decimal_spread: 0.001,
    };
    for (name, action) in [
        ("construct", BondOperation::Construct {}),
        ("requirements", BondOperation::Requirements {}),
        ("cashflows", BondOperation::Cashflows {}),
        ("price", BondOperation::Price { method }),
    ] {
        add(
            c,
            format!("frn.{name}"),
            inputs.clone(),
            LibraryOperation::Bond {
                bond: Box::new(b.clone()),
                context: ctx.clone(),
                projection: Some(p.key.clone()),
                action,
            },
        );
    }
    add(
        c,
        "fixings.inspect",
        inputs.clone(),
        LibraryOperation::Fixings {
            requirements: inputs.publications.clone(),
            context: ctx.fixing,
        },
    );
    let bond = bond(f.bond("bond")?.terms().clone())?;
    let trade = TradeId::new("library.lifecycle.bond")?;
    let steps = [
        ("rate", "2024-04-02", l::LifecycleAction::ContractStep),
        ("principal", "2024-07-02", l::LifecycleAction::ContractStep),
        ("coupon", "2024-07-02", l::LifecycleAction::ContractualCash),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (id, d, action))| {
        Ok(LifecycleStep {
            event: l::LifecycleEvent {
                event_id: l::EventId::new(id)?,
                trade_id: trade.clone(),
                expected_revision: i as u64,
                effective_date: date(d)?,
                known_at: at(&format!("{d}T23:59:59Z"))?,
                action,
            },
            credit_rounding: None,
            futures: None,
            delivery: None,
        })
    })
    .collect::<Result<Vec<_>, Error>>()?;
    let input = LifecycleInput {
        trade_id: trade,
        version: Version::new(1)?,
        instrument: LifecycleInstrumentInput::Bond { bond, credit: None },
        position: l::LifecyclePosition::BondFace(n("2000")?),
        known_at: at("2024-01-02T23:59:59Z")?,
        history: vec![],
        events: steps,
    };
    add(
        c,
        "lifecycle.stepped_amortizing",
        supplied_resources(&input, vec![], &[])?,
        LibraryOperation::Lifecycle {
            input: Box::new(input),
        },
    );
    for id in ["irs", "ois"] {
        let terms: convex_rates::SwapTerms = serde_json::from_value(f.data[id].clone())?;
        let known_at = at(&format!("{}T23:59:59Z", terms.effective_date))?;
        let input = LifecycleInput {
            trade_id: terms.trade_id.clone(),
            version: terms.version,
            instrument: LifecycleInstrumentInput::Swap { terms },
            position: l::LifecyclePosition::TradeCopies(n("2")?),
            known_at,
            history: vec![],
            events: vec![],
        };
        add(
            c,
            format!("lifecycle.{id}"),
            supplied_resources(&input, vec![], &[])?,
            LibraryOperation::Lifecycle {
                input: Box::new(input),
            },
        );
    }
    Ok(())
}
fn cash_roll(c: &mut Vec<Case>) -> Result<(), Error> {
    let p = future::BondPack::new();
    let universe = CashHedgeUniverse {
        stamp: future::stamp("cash-control-universe"),
        currency: Currency::USD,
        candidates: vec![CashHedgeCandidate {
            instrument: p.basket.bonds[0].instrument.clone(),
            quote: future::stamp("cash-control-quote"),
            currency: Currency::USD,
            settlement: future::d("2026-10-08"),
            lot_face: n("1000")?,
            bid_per100: Some(n("94")?),
            ask_per100: Some(n("94.1")?),
            bid_size_face: Some(n("1000000")?),
            ask_size_face: Some(n("1000000")?),
            financing: Some(p.financing[0].clone()),
            borrow: None,
            costs: Some(CashTradingCost {
                stamp: future::stamp("cash-control-cost"),
                currency_per100_face: n("0.01")?,
            }),
        }],
    };
    let input = CashHedgeInput {
        universe,
        instrument: p.basket.bonds[0].instrument.clone(),
        security_stamp: p.bond_stamps[0].clone(),
        bond: bond(p.bonds[0].terms().clone())?,
        request: CashHedgeRequest {
            side: CashHedgeSide::Buy,
            face: n("100000")?,
            owned_face: n("100000")?,
            horizon: future::d("2026-12-08"),
        },
        discount: StampedCurve {
            curve: p.discount.recipe().key.clone(),
            stamp: p.curve_stamp.clone(),
        },
        own_cash: p.own[0].clone(),
        borrow_fee_basis: None,
        context: future::ctx(),
    };
    let inputs = supplied_resources(&input, vec![p.discount.recipe().clone()], &[])?;
    add(
        c,
        "cash_hedge.price",
        inputs.clone(),
        LibraryOperation::CashHedge {
            input: Box::new(input.clone()),
            action: CashHedgeOperation::Price {},
        },
    );
    add(
        c,
        "cash_hedge.risk",
        inputs,
        LibraryOperation::CashHedge {
            input: Box::new(input),
            action: CashHedgeOperation::Risk { bump: n("0.0001")? },
        },
    );
    let p = future::RatePack::new(&future::fixture()["rates"]["cases"][0]);
    let front = p.contract;
    let mut back = front.clone();
    back.instrument_id = "CME-SR3Z6-control".to_string().try_into()?;
    back.stamp = future::stamp(back.instrument_id.as_str());
    back.contract_code = "SR3Z6-control".into();
    back.contract_month.month = 12;
    let factor: RiskFactorKey = serde_json::from_value(
        serde_json::json!({"family":"interest_rate","target":{"kind":"curve","curve_id":"USD.SOFR"},"tenor":"parallel","currency":"USD","quote_type":"zero_rate","basis":"zero_node","convention_version":1,"model_version":"explicit-futures-risk-v1"}),
    )?;
    let leg = |contract: &FuturesContract, points: &str, risk: &str| -> Result<RollLeg, Error> {
        Ok(RollLeg {
            contract: contract.clone(),
            basket: None,
            quote: future::mark(contract, points, "2026-10-07", f::FuturesMarkKind::Trade),
            risk: f::FuturesRollRisk {
                contract: future::pin(contract),
                factor: factor.clone(),
                per_contract_currency_per_bp: Money::new(n(risk)?, Currency::USD),
                stamp: future::stamp("roll-risk-control"),
            },
            costs_per_contract: n("1.5")?,
            costs_stamp: future::stamp("roll-cost-control"),
        })
    };
    add(
        c,
        "future.roll",
        LibraryInputs::empty(),
        LibraryOperation::FuturesMath {
            action: Box::new(FuturesMathOperation::Roll {
                front: leg(&front, "96", "-20")?,
                back: Box::new(leg(&back, "95.5", "-25")?),
                contracts: 3,
                rounding: f::FuturesRounding::NearestAwayFromZero,
                context: future::ctx(),
            }),
        },
    );
    Ok(())
}
fn credit_hedge(c: &mut Vec<Case>) -> Result<(), Error> {
    let data = credit::inputs()?;
    let recipe = credit::recipe(&data)?;
    let terms = recipe.helpers[5].terms.clone();
    let bond = cr::BondCreditIdentity {
        instrument_id: InstrumentId::new("Supplied.Bond")?,
        instrument_version: Version::new(1)?,
        reference: terms.reference.document.clone(),
        entity_id: terms.reference.entity_id.clone(),
        seniority: terms.reference.seniority,
        currency: terms.currency,
        maturity: date("2032-06-20")?,
    };
    let basis = cr::BondBasisInput {
        pin: terms.reference.restructuring_clause.clone(),
        currency: bond.currency,
        entity_id: bond.entity_id.clone(),
        seniority: bond.seniority,
        recovery_fraction: terms.reference.recovery_fraction,
        model_version: ModelId::new("continuous-zero-residual.supplied.v1")?,
        reference_date: recipe.curve.reference_date,
        dated_spreads: vec![
            (
                recipe.curve.reference_date,
                MarketQuote {
                    value: 0.0,
                    unit: QuoteUnit::BasisPoints,
                },
            ),
            (
                bond.maturity,
                MarketQuote {
                    value: 20.0,
                    unit: QuoteUnit::BasisPoints,
                },
            ),
        ],
        liquidity_assumption: terms.reference.document.clone(),
    };
    let mapping = cr::BondCdsMapping {
        pin: terms.reference.document.clone(),
        bond: bond.clone(),
        cds_reference: terms.reference.clone(),
        credit_curve: recipe.curve.key.clone(),
        basis: basis.pin.clone(),
        liquidity_assumption: basis.liquidity_assumption.clone(),
    };
    let input = BondCdsHedgeInput {
        mapping: Some(mapping),
        bond,
        cds: terms,
        credit: recipe.curve.key.clone(),
        basis,
    };
    let mut inputs =
        supplied_resources(&input, vec![data.discount_recipe], &data.calendar_holidays)?;
    inputs.calibration = Some(Box::new(CalibrationInput {
        rates: vec![],
        credit: vec![recipe],
        cash: vec![],
        rate_context: None,
        controls: FitNumericalControls::default(),
    }));
    add(
        c,
        "bond_cds.admission",
        inputs,
        LibraryOperation::BondCdsHedge {
            input: Box::new(input),
        },
    );
    Ok(())
}
fn quoted_risk(c: &mut Vec<Case>) -> Result<(), Error> {
    let base = c
        .iter()
        .find(|c| c.id == "bond.yield")
        .ok_or("missing original bond yield")?
        .clone();
    let LibraryOperation::Bond {
        bond,
        context,
        projection,
        action: BondOperation::Price { method },
    } = base.request.operation
    else {
        return Err("wrong source".into());
    };
    let key = cash::key(
        QuoteType::Yield,
        q::RiskCoordinate::Raw {
            input_id: "supplied.exact.yield".into(),
            version: Version::new(1)?,
        },
        QuoteUnit::DecimalRate,
        q::RiskResponse::FrozenCurves,
        "spot",
    )?;
    let position = cash::position(
        "-10000",
        &bond.terms.face_value.to_string(),
        q::RiskQuantityUnit::Notional,
    )?;
    add(
        c,
        "bond.quote_risk",
        base.request.inputs,
        LibraryOperation::Bond {
            bond,
            context,
            projection,
            action: BondOperation::QuoteRisk {
                method,
                key: Box::new(key.clone()),
                position: position.clone(),
                snapshot_id: cash::snapshot()?,
                settings: cash::settings(0.0001)?,
            },
        },
    );
    let result = convex::library::execute(&c.last().ok_or("missing risk")?.request)?;
    let sensitivity: q::QuoteSensitivity = serde_json::from_value(result["data"].clone())?;
    let trade = convex_portfolio::signed_risk::TradeSignedRisk {
        trade_id: TradeId::new("supplied.risk.trade")?,
        trade_version: Version::new(1)?,
        entity: EntityId::new("supplied.entity")?,
        currency: Currency::USD,
        gross_exposure: Some(n("10000")?),
        required_factors: vec![key],
        outcomes: vec![convex_portfolio::signed_risk::TradeFactorRisk::Present {
            sensitivity: Box::new(sensitivity),
        }],
    };
    add(
        c,
        "aggregate.complete",
        LibraryInputs::empty(),
        LibraryOperation::Aggregate {
            input: Box::new(AggregateInput {
                trades: vec![trade],
                snapshot_id: cash::snapshot()?,
                scope: convex_portfolio::signed_risk::RiskEntityScope::Single {
                    entity: EntityId::new("supplied.entity")?,
                },
                reporting_currency: Currency::USD,
                fx: vec![],
            }),
        },
    );
    let source = c
        .iter()
        .find(|c| c.id == "optional.risk")
        .ok_or("missing optional")?
        .clone();
    let LibraryOperation::OptionalBond { input, .. } = source.request.operation else {
        return Err("wrong optional".into());
    };
    let key = cash::key(
        QuoteType::OasSpread,
        q::RiskCoordinate::Raw {
            input_id: "OPTIONAL.CVX024".into(),
            version: Version::new(1)?,
        },
        QuoteUnit::DecimalRate,
        q::RiskResponse::FrozenCurves,
        "spot",
    )?;
    add(
        c,
        "optional.quote_risk",
        source.request.inputs,
        LibraryOperation::OptionalBond {
            input,
            action: OptionalOperation::QuoteRisk {
                oas_decimal: 0.00123456,
                key: Box::new(key),
                position: cash::position("1", "1", q::RiskQuantityUnit::TradeCopies)?,
                snapshot_id: cash::snapshot()?,
                settings: cash::settings(0.01)?,
                native_settings: o::RiskSettings {
                    absolute_duration_tolerance: 0.02,
                    relative_duration_tolerance: 0.01,
                    absolute_convexity_tolerance: 0.25,
                    relative_convexity_tolerance: 0.01,
                },
            },
        },
    );
    Ok(())
}
fn model_calibration(c: &mut Vec<Case>) -> Result<(), Error> {
    let data: Value = serde_json::from_str(include_str!(
        "../../../../convex-analytics/tests/fixtures/optional-bonds.json"
    ))?;
    let source = c
        .iter()
        .find(|c| c.id == "optional.price")
        .ok_or("missing optional")?
        .clone();
    let LibraryOperation::OptionalBond { mut input, .. } = source.request.operation else {
        return Err("wrong optional".into());
    };
    let quotes = data["swaption_cases"]
        .as_array()
        .ok_or("missing swaption inputs")?
        .iter()
        .map(|v| -> Result<o::SwaptionCalibrationQuote, Error> {
            let expiry = date("2025-01-01")?
                .checked_add_days(v["expiry_years"].as_i64().ok_or("expiry")? * 365)?;
            let end = expiry.checked_add_days(v["tail_years"].as_i64().ok_or("tail")? * 365)?;
            Ok(o::SwaptionCalibrationQuote {
                id: v["id"].as_str().ok_or("id")?.into(),
                exercise_date: expiry,
                fixed_schedule: ScheduleSpec {
                    start_date: expiry,
                    end_date: end,
                    frequency: Frequency::Annual,
                    stub_type: StubType::ShortLast,
                    end_of_month: false,
                    first_regular_date: None,
                    penultimate_date: None,
                },
                dates: DateRules::default(),
                fixed_day_count: Dc::Act365Fixed,
                strike_rate: v["strike"].as_f64().ok_or("strike")?,
                side: serde_json::from_value(v["side"].clone())?,
                volatility: if v["convention"] == "normal" {
                    o::SwaptionVolatility::Normal {
                        annualized_decimal: v["volatility"].as_f64().ok_or("vol")?,
                    }
                } else {
                    o::SwaptionVolatility::ShiftedLognormal {
                        annualized_decimal: v["volatility"].as_f64().ok_or("vol")?,
                        shift: v["shift"].as_f64().ok_or("shift")?,
                    }
                },
                native_tolerance: 2e-7,
                scale: 0.01,
                weight: 1.0,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    input.model = OptionalModelInput::Swaptions {
        key: CurveKey {
            id: "supplied.calibrated.hw1f".into(),
            version: "1".into(),
        },
        valuation_date: date("2025-01-01")?,
        currency: Currency::USD,
        discount: input.discount.clone(),
        calendar: input.bond.calendars.exercise.clone(),
        mean_reversion: 0.03,
        quotes,
        settings: o::SigmaCalibrationSettings {
            initial_sigma: 0.008,
            lower_sigma: 0.001,
            upper_sigma: 0.05,
            max_iterations: 100,
            objective_tolerance: 2e-5,
            jacobian_step: 1e-6,
            minimum_singular_ratio: 1e-10,
            svd_iterations: 1000,
        },
    };
    add(
        c,
        "optional.calibrated_model",
        source.request.inputs,
        LibraryOperation::OptionalBond {
            input,
            action: OptionalOperation::Price { oas_decimal: 0.0 },
        },
    );
    Ok(())
}
fn bad(
    c: &mut Vec<Case>,
    source: &str,
    id: &str,
    code: LibraryErrorCode,
    change: impl FnOnce(&mut LibraryRequest),
) -> Result<(), Error> {
    let mut case = c
        .iter()
        .find(|c| c.id == source)
        .ok_or("negative source missing")?
        .clone();
    case.id = id.into();
    case.expected_error = Some(code);
    change(&mut case.request);
    c.push(case);
    Ok(())
}
fn negative(c: &mut Vec<Case>) -> Result<(), Error> {
    bad(
        c,
        "bond.construct",
        "bond.missing_calendar",
        LibraryErrorCode::MissingInput,
        |r| r.inputs.calendars.clear(),
    )?;
    bad(
        c,
        "bond.construct",
        "bond.invalid_profile",
        LibraryErrorCode::InvalidInput,
        |r| {
            if let LibraryOperation::Bond { bond, .. } = &mut r.operation {
                bond.terms.profiles.notional[0].value = Decimal::NEGATIVE_ONE
            }
        },
    )?;
    bad(
        c,
        "bond.yield_inverse",
        "bond.no_root",
        LibraryErrorCode::NoRoot,
        |r| {
            if let LibraryOperation::Bond {
                action: BondOperation::YieldInverse { target, .. },
                ..
            } = &mut r.operation
            {
                *target = w::BondPriceTarget::Dirty(10000.0)
            }
        },
    )?;
    bad(
        c,
        "bond.yield_inverse",
        "bond.nonunique",
        LibraryErrorCode::NonUnique,
        |r| {
            if let LibraryOperation::Bond {
                bond,
                context,
                action: BondOperation::YieldInverse { target, .. },
                ..
            } = &mut r.operation
            {
                context.settlement = bond.terms.schedule.end_date;
                *target = w::BondPriceTarget::Dirty(0.0)
            }
        },
    )?;
    bad(
        c,
        "bond.quote_risk",
        "bond.risk_quantity",
        LibraryErrorCode::InvalidInput,
        |r| {
            if let LibraryOperation::Bond {
                action: BondOperation::QuoteRisk { position, .. },
                ..
            } = &mut r.operation
            {
                position.reference_quantity = ExactDecimal::new(Decimal::ONE)
            }
        },
    )?;
    bad(
        c,
        "frn.price",
        "frn.missing_projection",
        LibraryErrorCode::MissingInput,
        |r| {
            if let LibraryOperation::Bond { projection, .. } = &mut r.operation {
                *projection = None
            }
        },
    )?;
    bad(
        c,
        "frn.price",
        "frn.not_applicable_yield",
        LibraryErrorCode::NotApplicable,
        |r| {
            if let LibraryOperation::Bond { action, .. } = &mut r.operation {
                *action = BondOperation::Price {
                    method: BondMethod::Yield {
                        decimal_rate: 0.03,
                        convention: w::DatedYieldConvention {
                            day_count: Dc::Act365Fixed,
                            compounding: Compounding::SemiAnnual,
                        },
                    },
                }
            }
        },
    )?;
    bad(
        c,
        "cds.price",
        "cds.missing_credit",
        LibraryErrorCode::MissingInput,
        |r| r.inputs.calibration = None,
    )?;
    bad(
        c,
        "bond_cds.admission",
        "bond_cds.missing_mapping",
        LibraryErrorCode::InvalidInput,
        |r| {
            if let LibraryOperation::BondCdsHedge { input } = &mut r.operation {
                input.mapping = None
            }
        },
    )?;
    bad(
        c,
        "optional.quote_risk",
        "optional.unresolved_risk",
        LibraryErrorCode::NoConvergence,
        |r| {
            if let LibraryOperation::OptionalBond {
                action: OptionalOperation::QuoteRisk { settings, .. },
                ..
            } = &mut r.operation
            {
                settings.bump = 0.0001
            }
        },
    )?;
    bad(
        c,
        "lifecycle.zero",
        "lifecycle.bad_order",
        LibraryErrorCode::InvalidInput,
        |r| {
            if let LibraryOperation::Lifecycle { input } = &mut r.operation {
                input.events[0].event.expected_revision = 3
            }
        },
    )?;
    bad(
        c,
        "future.round_hedge",
        "hedge.zero_risk",
        LibraryErrorCode::InvalidInput,
        |r| {
            if let LibraryOperation::RoundHedge {
                per_contract_currency_per_bp,
                ..
            } = &mut r.operation
            {
                *per_contract_currency_per_bp = ExactDecimal::new(Decimal::ZERO)
            }
        },
    )?;
    bad(
        c,
        "curve.forward",
        "curve.unsupported_tenor_basis",
        LibraryErrorCode::Unsupported,
        |r| {
            if let LibraryOperation::CurveQuery {
                query: CurveQuery::ForwardRate { day_count, .. },
                ..
            } = &mut r.operation
            {
                *day_count = Dc::Act360
            }
        },
    )?;
    Ok(())
}
