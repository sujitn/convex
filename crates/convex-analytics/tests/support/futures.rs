#![allow(dead_code)]
use convex_analytics::futures::*;
use convex_bonds::contracts::{BondContract, BondTerms};
use convex_core::{
    calendars::{Calendar, DynamicCalendar, WeekendType},
    contracts::*,
    fixings::*,
    futures::*,
    Currency, Date,
};
use convex_curves::{contracts::*, ExtrapolationMethod, InterpolationMethod};
use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;
pub fn e(s: &str) -> ExactDecimal {
    ExactDecimal::parse(s).unwrap()
}
pub fn d(s: &str) -> BusinessDate {
    BusinessDate::parse(s).unwrap()
}
pub fn t(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse(s).unwrap()
}
pub fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/futures/cme-2026-09-24.json"
    ))
    .unwrap()
}
pub fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/futures/valuation.json"
    ))
    .unwrap()
}
pub fn exchange() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/futures/exchange-settlement.json"
    ))
    .unwrap()
}
pub fn ctx() -> EligibilityContext {
    EligibilityContext {
        as_of: t("2026-10-07T12:00:00Z"),
        known_at: t("2026-10-07T12:00:00Z"),
    }
}
pub fn stamp(id: &str) -> ReferenceStamp {
    ReferenceStamp {
        id: id.to_string().try_into().unwrap(),
        version: Version::new(1).unwrap(),
        source: "engineering".to_string().try_into().unwrap(),
        source_at: t("2026-09-01T00:00:00Z"),
        available_at: t("2026-09-01T00:00:00Z"),
        valid_until: t("2027-01-01T00:00:00Z"),
        quality: ReferenceQuality::Synthetic,
        evidence:
            "Explicit synthetic engineering input; no actual market/source availability claim"
                .into(),
    }
}
pub fn curve(role: CurveRole, rate: f64, on: Date) -> BuiltCurve {
    let key = CurveKey {
        id: "curve-control".into(),
        version: "1".into(),
    };
    let recipe = CurveRecipe {
        key: key.clone(),
        role,
        currency: Currency::USD,
        reference_date: on,
        time_basis: convex_core::daycounts::DayCountConvention::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: convex_core::types::Compounding::Continuous,
        },
        interpolation: InterpolationMethod::Linear,
        extrapolation: ExtrapolationMethod::Flat,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Tenor(0.0001),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
            CurveNode {
                pillar: CurvePoint::Tenor(50.),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
        ]),
    };
    build_curve_set(&[recipe], &CurveSet::new())
        .unwrap()
        .curves
        .get(&key)
        .unwrap()
        .as_ref()
        .clone()
}
pub fn pin(c: &FuturesContract) -> InstrumentPin {
    InstrumentPin {
        id: c.instrument_id.clone(),
        version: c.stamp.version,
    }
}
pub fn rules(c: &FuturesContract, increment: Option<&str>) -> FuturesClearingRules {
    FuturesClearingRules {
        contract: pin(c),
        stamp: stamp(&format!("{}-rules", c.instrument_id)),
        money_method: "rounded_contract_values".into(),
        final_rate_increment: increment.map(e),
        final_rate_rounding: increment.map(|_| FuturesRounding::NearestUp),
        fixing_selection: increment.map(|_| FuturesFixingSelection::FirstPublished),
    }
}
pub fn mark(c: &FuturesContract, points: &str, date: &str, kind: FuturesMarkKind) -> FuturesMark {
    FuturesMark {
        contract: pin(c),
        stamp: stamp(&format!("{}-mark-{}", c.instrument_id, date)),
        date: d(date),
        price_points: e(points),
        kind,
    }
}
pub struct BondPack {
    pub contract: FuturesContract,
    pub basket: DeliveryBasket,
    pub calendar: DynamicCalendar,
    pub calendar_stamp: ReferenceStamp,
    pub market: FuturesMark,
    pub bonds: Vec<BondContract>,
    pub bond_stamps: Vec<ReferenceStamp>,
    pub quote_stamps: Vec<ReferenceStamp>,
    pub financing: Vec<FinancingInput>,
    pub own: Vec<Option<FuturesCarryRate>>,
    pub coupon: Vec<FuturesCouponCarry>,
    pub prices: Vec<ExactDecimal>,
    pub discount: BuiltCurve,
    pub curve_stamp: ReferenceStamp,
}
impl BondPack {
    pub fn new() -> Self {
        let f = fixture();
        let r = reference();
        let contract: FuturesContract = serde_json::from_value(r["contract"].clone()).unwrap();
        let basket: DeliveryBasket = serde_json::from_value(r["basket"].clone()).unwrap();
        let members = f["bond"]["members"].as_array().unwrap();
        let bonds = members
            .iter()
            .map(|m| {
                serde_json::from_value::<BondTerms>(m["terms"].clone())
                    .unwrap()
                    .build()
                    .unwrap()
            })
            .collect();
        let own: Vec<_> = members
            .iter()
            .enumerate()
            .map(|(i, m)| {
                if e(m["haircut"].as_str().unwrap()).decimal().is_zero() {
                    None
                } else {
                    Some(FuturesCarryRate {
                        stamp: stamp(&format!("own-cash-{i}")),
                        start: d("2026-10-08"),
                        end: d("2026-12-31"),
                        decimal_rate: e(m["own_cash_rate"].as_str().unwrap()),
                        day_count: convex_core::daycounts::DayCountConvention::Act365Fixed,
                    })
                }
            })
            .collect();
        let coupon = members
            .iter()
            .enumerate()
            .map(|(i, m)| {
                if m["coupon_policy"] == "held_cash" {
                    FuturesCouponCarry::HeldCash {
                        evidence: "Explicit uninvested-coupon assumption".into(),
                    }
                } else {
                    FuturesCouponCarry::Reinvested {
                        rate: FuturesCarryRate {
                            stamp: stamp(&format!("reinvest-{i}")),
                            start: d("2026-10-08"),
                            end: d("2026-12-31"),
                            decimal_rate: e(m["reinvestment_rate"].as_str().unwrap()),
                            day_count: convex_core::daycounts::DayCountConvention::Act365Fixed,
                        },
                    }
                }
            })
            .collect();
        let financing = members
            .iter()
            .enumerate()
            .map(|(i, m)| FinancingInput {
                stamp: stamp(&format!("repo-{i}")),
                instrument: basket.bonds[i].instrument.clone(),
                currency: Currency::USD,
                start: d("2026-10-08"),
                end: d("2026-12-31"),
                repo_decimal: e(m["repo"].as_str().unwrap()),
                day_count: convex_core::daycounts::DayCountConvention::Act360,
                haircut_fraction: e(m["haircut"].as_str().unwrap()),
            })
            .collect();
        let bond_stamps = basket
            .bonds
            .iter()
            .map(|b| stamp(b.instrument.id.as_str()))
            .collect();
        let quote_stamps = (0..members.len())
            .map(|i| stamp(&format!("cash-quote-{i}")))
            .collect();
        let prices = members
            .iter()
            .map(|m| e(m["clean"].as_str().unwrap()))
            .collect();
        let market = mark(&contract, "100", "2026-10-07", FuturesMarkKind::Trade);
        Self {
            contract,
            basket,
            calendar: DynamicCalendar::new(
                "explicit weekend delivery control",
                WeekendType::SaturdaySunday,
            ),
            calendar_stamp: stamp("CME-calendar-reference"),
            market,
            bonds,
            bond_stamps,
            quote_stamps,
            financing,
            own,
            coupon,
            prices,
            discount: curve(
                CurveRole::Discount {
                    collateral_currency: Currency::USD,
                },
                0.04,
                d("2026-10-07").date(),
            ),
            curve_stamp: stamp("curve-control"),
        }
    }
    pub fn deliverables(&self) -> Vec<BondFutureDeliverable<'_>> {
        (0..self.bonds.len())
            .map(|i| BondFutureDeliverable {
                security: FuturesBond {
                    instrument: &self.basket.bonds[i].instrument,
                    stamp: &self.bond_stamps[i],
                    bond: &self.bonds[i],
                },
                clean_price_per100: self.prices[i],
                quote_stamp: &self.quote_stamps[i],
                financing: &self.financing[i],
                own_cash: self.own[i].as_ref(),
                coupon_carry: &self.coupon[i],
            })
            .collect()
    }
    pub fn inputs<'a>(
        &'a self,
        deliverables: &'a [BondFutureDeliverable<'a>],
    ) -> BondFuturesInputs<'a> {
        BondFuturesInputs {
            contract: &self.contract,
            basket: &self.basket,
            delivery_calendar: FuturesCalendar {
                stamp: &self.calendar_stamp,
                calendar: &self.calendar,
            },
            settlement: d("2026-10-08"),
            delivery: d("2026-12-08"),
            market: &self.market,
            deliverables,
            selection: FuturesCtdMethod::MinimumNetBasis,
            context: ctx(),
        }
    }
}
pub struct RatePack {
    pub contract: FuturesContract,
    pub calendar: DynamicCalendar,
    pub calendar_stamp: ReferenceStamp,
    pub clearing: FuturesClearingRules,
    pub observations: Vec<FixingObservation>,
    pub history: FuturesFixings,
    pub fixing_stamp: ReferenceStamp,
    pub projection: BuiltCurve,
    pub projection_stamp: ReferenceStamp,
    pub context: EligibilityContext,
}
impl RatePack {
    pub fn new(case: &Value) -> Self {
        let mut c: FuturesContract =
            serde_json::from_value(reference()["rate_contract"].clone()).unwrap();
        c.stamp = stamp(c.instrument_id.as_str());
        if let FuturesSettlement::Rate {
            accrual: RateAccrual::Overnight { averaging, .. },
            convexity,
            ..
        } = &mut c.settlement
        {
            *averaging = serde_json::from_value(case["averaging"].clone()).unwrap();
            *convexity = FuturesConvexityPolicy::ZeroAssumption {
                evidence: "Explicit engineering zero convexity".into(),
            };
        }
        let mut calendar = DynamicCalendar::new(
            "explicit supplied rate calendar",
            WeekendType::SaturdaySunday,
        );
        calendar.add_holidays(
            fixture()["rates"]["holidays"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| d(s.as_str().unwrap()).date()),
        );
        let mut context = ctx();
        if case["coverage"] == "future" {
            context.as_of = t("2026-09-15T12:00:00Z");
            context.known_at = context.as_of;
        } else if case["coverage"] == "fixed" {
            context.as_of = t("2026-12-16T12:00:00Z");
            context.known_at = context.as_of;
        }
        let observations: Vec<_> = case["observations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["known"] == true)
            .map(|r| {
                let at = d(r["observation"].as_str().unwrap()).date();
                let mut next = at.add_days(1);
                while !calendar.is_business_day(next) {
                    next = next.add_days(1);
                }
                FixingObservation {
                    index_id: "USD.SOFR".to_string().try_into().unwrap(),
                    observation_date: at,
                    rate: e(r["decimal_rate"].as_str().unwrap()).decimal(),
                    published_at: t(&format!("{next}T08:00:00Z")),
                    known_at: t(&format!("{next}T08:00:00Z")),
                    revision: Version::new(1).unwrap(),
                    status: FixingStatus::Final,
                    source_id: "engineering".to_string().try_into().unwrap(),
                }
            })
            .collect();
        let history =
            FuturesFixings::new(observations.clone(), FuturesFixingSelection::FirstPublished)
                .unwrap();
        let clearing = rules(&c, Some("0.000001"));
        let mut projection_stamp = stamp("curve-control");
        projection_stamp.source_at = t("2026-09-01T00:00:00Z");
        projection_stamp.available_at = projection_stamp.source_at;
        Self {
            contract: c,
            calendar,
            calendar_stamp: stamp("CME-calendar-reference"),
            clearing,
            observations,
            history,
            fixing_stamp: stamp("rates-history"),
            projection: curve(
                CurveRole::Projection {
                    index_id: "USD.SOFR".into(),
                },
                0.04,
                d("2026-09-01").date(),
            ),
            projection_stamp,
            context,
        }
    }
    pub fn inputs(&self) -> RateFuturesInputs<'_> {
        RateFuturesInputs {
            contract: &self.contract,
            calendar: FuturesCalendar {
                stamp: &self.calendar_stamp,
                calendar: &self.calendar,
            },
            clearing: &self.clearing,
            fixings: &self.history,
            fixing_stamp: &self.fixing_stamp,
            projection: Some(FuturesCurve {
                curve: &self.projection,
                stamp: &self.projection_stamp,
            }),
            convexity_model: None,
            context: self.context,
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        }
    }
    pub fn publication(
        &self,
        index: &IndexId,
        at: Date,
    ) -> Result<UtcTimestamp, convex_core::cashflows::CashflowError> {
        assert_eq!(index.as_str(), "USD.SOFR");
        let mut next = at.add_days(1);
        while !self.calendar.is_business_day(next) {
            next = next.add_days(1);
        }
        Ok(t(&format!("{next}T08:00:00Z")))
    }
}
// Only reference comparisons truncate Decimal(60) strings at Decimal capacity
// (far below the declared comparison bounds). Original reference bytes remain.
// all native economic input strings still pass through the exact parser above.
pub fn reference_decimal(value: &Value) -> Decimal {
    let s = value.as_str().unwrap();
    if s.trim_start_matches('-').starts_with("0E-") {
        return Decimal::ZERO;
    }
    let Some((whole, fraction)) = s.split_once('.') else {
        return Decimal::from_str(s).unwrap();
    };
    let digits = whole.trim_start_matches('-').trim_start_matches('0').len();
    let keep = fraction.len().min(28 - digits);
    Decimal::from_str(&format!("{whole}.{}", &fraction[..keep])).unwrap()
}
pub fn close_decimal(actual: Decimal, expected: &Value, bound: &str) {
    let expected = reference_decimal(expected);
    assert!(
        (actual - expected).abs() <= e(bound).decimal(),
        "actual {actual} expected {expected}"
    );
}
pub fn close_risk(actual: Decimal, expected: &Value) {
    let expected = reference_decimal(expected);
    let tolerance = e("0.000001").decimal() + expected.abs() * e("0.0000000001").decimal();
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual} expected {expected} bound {tolerance}"
    );
}
