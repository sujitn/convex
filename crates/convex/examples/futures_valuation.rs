//! Pure consumer of complete supplied futures economics; all inputs are controls.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use convex::{
    bond_contracts::{BondContract, BondTerms},
    contracts::*,
    curve_contracts::*,
    fixings::*,
    futures::*,
    futures_reference::*,
    Currency, Date,
};
use convex_core::{calendars::Calendar, cashflows::CashflowError};
use serde_json::{json, Value};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn e(s: &str) -> Result<ExactDecimal> {
    Ok(ExactDecimal::parse(s)?)
}
fn d(s: &str) -> Result<BusinessDate> {
    Ok(BusinessDate::parse(s)?)
}
fn t(s: &str) -> Result<UtcTimestamp> {
    Ok(UtcTimestamp::parse(s)?)
}
fn stamp(id: &str) -> Result<ReferenceStamp> {
    Ok(ReferenceStamp {
        id: id.to_string().try_into()?,
        version: Version::new(1)?,
        source: "engineering".to_string().try_into()?,
        source_at: t("2026-09-01T00:00:00Z")?,
        available_at: t("2026-09-01T00:00:00Z")?,
        valid_until: t("2027-01-01T00:00:00Z")?,
        quality: ReferenceQuality::Synthetic,
        evidence: "Explicit synthetic engineering control, no live source claim".into(),
    })
}
struct SuppliedCalendar {
    holidays: Vec<Date>,
}
impl Calendar for SuppliedCalendar {
    fn name(&self) -> &'static str {
        "supplied engineering calendar v1"
    }
    fn is_business_day(&self, at: Date) -> bool {
        !at.is_weekend() && !self.holidays.contains(&at)
    }
}
fn publication(
    calendar: &SuppliedCalendar,
    at: Date,
) -> std::result::Result<UtcTimestamp, CashflowError> {
    for offset in 1..=366 {
        let next = at.add_days(offset);
        if calendar.is_business_day(next) {
            return UtcTimestamp::parse(&format!("{next}T08:00:00Z")).map_err(|error| {
                CashflowError::InvalidTerms {
                    field: "publication",
                    reason: error.to_string(),
                }
            });
        }
    }
    Err(CashflowError::BoundExceeded(
        "supplied publication calendar",
    ))
}
struct SuppliedMember {
    bond: BondContract,
    security: ReferenceStamp,
    quote: ReferenceStamp,
    financing: FinancingInput,
    own: Option<FuturesCarryRate>,
    coupon: FuturesCouponCarry,
    clean: ExactDecimal,
}
fn main() -> Result<()> {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/futures/cme-2026-09-24.json"
    ))?;
    let controls: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/futures/valuation.json"
    ))?;
    let contract: FuturesContract = serde_json::from_value(reference["contract"].clone())?;
    let basket: DeliveryBasket = serde_json::from_value(reference["basket"].clone())?;
    let context = EligibilityContext {
        as_of: t("2026-10-07T12:00:00Z")?,
        known_at: t("2026-10-07T12:00:00Z")?,
    };
    let calendar = SuppliedCalendar {
        holidays: vec![
            d("2026-10-12")?.date(),
            d("2026-11-11")?.date(),
            d("2026-11-26")?.date(),
        ],
    };
    let calendar_stamp = stamp("CME-calendar-reference")?;
    let rows = controls["bond"]["members"]
        .as_array()
        .ok_or("missing complete member controls")?;
    if rows.len() != basket.bonds.len() {
        return Err("incomplete controls".into());
    }
    let mut supplied = Vec::new();
    for (m, b) in rows.iter().zip(&basket.bonds) {
        let id = b.instrument.id.as_str();

        let haircut: ExactDecimal = serde_json::from_value(m["haircut"].clone())?;
        let financing = FinancingInput {
            stamp: stamp(&format!("repo-{id}"))?,
            instrument: b.instrument.clone(),
            currency: Currency::USD,
            start: d("2026-10-08")?,
            end: d("2026-12-31")?,
            repo_decimal: serde_json::from_value(m["repo"].clone())?,
            day_count: convex::DayCountConvention::Act360,
            haircut_fraction: haircut,
        };
        let carry = |prefix: &str, value: Value| -> Result<FuturesCarryRate> {
            Ok(FuturesCarryRate {
                stamp: stamp(&format!("{prefix}-{id}"))?,
                start: d("2026-10-08")?,
                end: d("2026-12-31")?,
                decimal_rate: serde_json::from_value(value)?,
                day_count: convex::DayCountConvention::Act365Fixed,
            })
        };
        let own = if haircut.decimal().is_zero() {
            None
        } else {
            Some(carry("own-cash", m["own_cash_rate"].clone())?)
        };
        let coupon = if m["coupon_policy"] == "held_cash" {
            FuturesCouponCarry::HeldCash {
                evidence: "Supplied uninvested coupon control".into(),
            }
        } else {
            FuturesCouponCarry::Reinvested {
                rate: carry("reinvestment", m["reinvestment_rate"].clone())?,
            }
        };
        supplied.push(SuppliedMember {
            bond: serde_json::from_value::<BondTerms>(m["terms"].clone())?.build()?,
            security: stamp(id)?,
            quote: stamp(&format!("quote-{id}"))?,
            financing,
            own,
            coupon,
            clean: serde_json::from_value(m["clean"].clone())?,
        });
    }
    let deliverables = basket
        .bonds
        .iter()
        .zip(&supplied)
        .map(|(b, s)| BondFutureDeliverable {
            security: FuturesBond {
                instrument: &b.instrument,
                stamp: &s.security,
                bond: &s.bond,
            },
            clean_price_per100: s.clean,
            quote_stamp: &s.quote,
            financing: &s.financing,
            own_cash: s.own.as_ref(),
            coupon_carry: &s.coupon,
        })
        .collect::<Vec<_>>();
    let market = FuturesMark {
        contract: InstrumentPin {
            id: contract.instrument_id.clone(),
            version: contract.stamp.version,
        },
        stamp: stamp("market-control")?,
        date: d("2026-10-07")?,
        price_points: e("100")?,
        kind: FuturesMarkKind::Trade,
    };
    let inputs = BondFuturesInputs {
        contract: &contract,
        basket: &basket,
        delivery_calendar: FuturesCalendar {
            calendar: &calendar,
            stamp: &calendar_stamp,
        },
        settlement: d("2026-10-08")?,
        delivery: d("2026-12-08")?,
        market: &market,
        deliverables: &deliverables,
        selection: FuturesCtdMethod::MinimumNetBasis,
        context,
    };
    let bond = price_bond_future(&inputs)?;
    let repo_risk = bond_futures_risk(&inputs, BondFuturesRiskAxis::RepoRates, 3, e("0.0001")?)?;

    let mut rate: FuturesContract = serde_json::from_value(reference["rate_contract"].clone())?;
    rate.stamp = stamp(rate.instrument_id.as_str())?;
    if let FuturesSettlement::Rate { convexity, .. } = &mut rate.settlement {
        *convexity = FuturesConvexityPolicy::ZeroAssumption {
            evidence: "Explicit supplied zero convexity control".into(),
        };
    }
    let case = controls["rates"]["cases"]
        .as_array()
        .and_then(|r| {
            r.iter()
                .find(|r| r["averaging"] == "compounded" && r["coverage"] == "partial")
        })
        .ok_or("missing partial fixing controls")?;
    let observations = case["observations"]
        .as_array()
        .ok_or("missing rate controls")?
        .iter()
        .filter(|r| r["known"] == true)
        .map(|r| -> Result<FixingObservation> {
            let at: BusinessDate = serde_json::from_value(r["observation"].clone())?;
            let pub_at = publication(&calendar, at.date())?;
            Ok(FixingObservation {
                index_id: "USD.SOFR".to_string().try_into()?,
                observation_date: at.date(),
                rate: serde_json::from_value::<ExactDecimal>(r["decimal_rate"].clone())?.decimal(),
                published_at: pub_at,
                known_at: pub_at,
                revision: Version::new(1)?,
                status: FixingStatus::Final,
                source_id: "engineering".to_string().try_into()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let history = FuturesFixings::new(observations, FuturesFixingSelection::FirstPublished)?;
    let rules = FuturesClearingRules {
        contract: InstrumentPin {
            id: rate.instrument_id.clone(),
            version: rate.stamp.version,
        },
        stamp: stamp("clearing-control")?,
        money_method: "rounded_contract_values".into(),
        final_rate_increment: Some(e("0.000001")?),
        final_rate_rounding: Some(FuturesRounding::NearestUp),
        fixing_selection: Some(FuturesFixingSelection::FirstPublished),
    };
    let recipe: CurveRecipe = serde_json::from_value(
        json!({"key":{"id":"projection-control","version":"1"},"role":{"kind":"Projection","index_id":"USD.SOFR"},"currency":"USD","reference_date":"2026-09-01","time_basis":"Act365Fixed","representation":{"ZeroRates":{"compounding":"Continuous"}},"interpolation":"Linear","extrapolation":"Flat","dependencies":[],"calibration":"Direct","source":{"Nodes":[{"pillar":{"Tenor":0.0001},"quote":{"value":0.04,"unit":"DecimalRate"}},{"pillar":{"Tenor":50.0},"quote":{"value":0.04,"unit":"DecimalRate"}}]}}),
    )?;
    let key = recipe.key.clone();
    let built = build_curve_set(&[recipe], &CurveSet::new())?;
    let projection = built.curves.get(&key)?;
    let projection_stamp = stamp("projection-control")?;
    let fixing_stamp = stamp("fixing-control")?;
    let inputs = RateFuturesInputs {
        contract: &rate,
        calendar: FuturesCalendar {
            calendar: &calendar,
            stamp: &calendar_stamp,
        },
        clearing: &rules,
        fixings: &history,
        fixing_stamp: &fixing_stamp,
        projection: Some(FuturesCurve {
            curve: projection,
            stamp: &projection_stamp,
        }),
        convexity_model: None,
        context,
        unpublished: UnpublishedFixingPolicy::Forecast,
        final_only: true,
    };
    let risk = rate_futures_risk(&inputs, |_, at| publication(&calendar, at), 3, e("0.0001")?)?;
    if bond.alternatives.len() != 10
        || bond.inputs.assessment.eligible
        || risk.base.known_days == 0
        || risk.base.projected_days == 0
        || risk.currency_per_forward_bp.value().decimal() >= e("0")?.decimal()
    {
        return Err("consumer postcondition failed".into());
    }
    println!(
        "{}",
        json!({"status":"passed","bond_contract":contract.contract_code,"actual_cf_members":bond.alternatives.len(),"selected":bond.selected,"fair_deliverable":bond.fair_deliverable,"fair_price_points":bond.fair_price_points,"repo_risk_currency_per_bp":repo_risk.reselected_sensitivity,"rate_price_points":risk.base.model_price_points,"known_days":risk.base.known_days,"projected_days":risk.base.projected_days,"remaining_forward_risk_currency_per_bp":risk.currency_per_forward_bp,"execution_status":risk.base.inputs.execution_status,"certified":false})
    );
    Ok(())
}
