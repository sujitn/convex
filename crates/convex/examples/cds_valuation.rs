//! Standalone CDS pricing and explicit running/clean/dirty quote conversion.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use convex::{
    credit::{
        convert_cds, price_cds, CalendarBinding, CdsCalendars, CdsQuote, CdsTerms,
        ConversionSettings, InitialPaymentState,
    },
    curve_contracts::{
        build_curve_set, CurveNode, CurvePoint, CurveRecipe, CurveRepresentation, CurveRole,
        CurveSet, CurveSource, MarketQuote, QuoteUnit,
    },
    Date,
};
use convex_core::calendars::{DynamicCalendar, WeekendType};
use serde_json::{json, Value};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/comparisons/cds-isda-1.8.3/inputs.json"
    ))?;
    let terms: CdsTerms = serde_json::from_value(fixture["template"].clone())?;
    let roundtrip: CdsTerms = serde_json::from_str(&serde_json::to_string(&terms)?)?;
    if terms != roundtrip {
        return Err("CDS economic terms changed in wire roundtrip".into());
    }
    let holidays: Vec<Date> = serde_json::from_value(fixture["calendar_holidays"].clone())?;
    let calendar = DynamicCalendar::from_dates(
        "supplied official NYM",
        WeekendType::SaturdaySunday,
        holidays,
    );
    let contract = terms.build(&CdsCalendars {
        premium: CalendarBinding {
            pin: &terms.premium_calendar,
            calendar: &calendar,
        },
        settlement: CalendarBinding {
            pin: &terms.settlement_calendar,
            calendar: &calendar,
        },
    })?;
    let discount: CurveRecipe = serde_json::from_value(fixture["discount_recipe"].clone())?;
    let mut survival = discount.clone();
    survival.key = terms.survival_curve.clone();
    survival.role = CurveRole::Credit {
        entity_id: terms.reference.entity_id.as_str().into(),
        seniority: terms.reference.seniority.as_str().into(),
        recovery: terms.reference.recovery_fraction.get(),
    };
    survival.representation = CurveRepresentation::SurvivalProbabilities;
    survival.source = CurveSource::Nodes(
        [0.0, 1.0, 5.0, 11.0]
            .into_iter()
            .map(|year| CurveNode {
                pillar: CurvePoint::Tenor(year),
                quote: MarketQuote {
                    value: (-0.03_f64 * year).exp(),
                    unit: QuoteUnit::SurvivalProbability,
                },
            })
            .collect(),
    );
    let curves = build_curve_set(&[discount, survival], &CurveSet::new())?.curves;
    let context = terms.trade_context(InitialPaymentState::NotContracted);
    let valuation = price_cds(&contract, &context, &curves)?;
    let converted = convert_cds(
        &contract,
        &context,
        &curves,
        CdsQuote::RunningSpread { decimal_rate: 0.01 },
        ConversionSettings::default(),
    )?;
    for quote in [
        CdsQuote::CleanUpfront {
            fraction: converted.valuation.clean_upfront_fraction,
        },
        CdsQuote::DirtyUpfront {
            fraction: converted.valuation.dirty_upfront_fraction,
        },
    ] {
        let back = convert_cds(
            &contract,
            &context,
            &curves,
            quote,
            ConversionSettings::default(),
        )?;
        if (back.conventional_spread_decimal - 0.01).abs() > 1e-10 {
            return Err("CDS native-unit quote roundtrip failed".into());
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"valuation":valuation,"conversion":converted,"qualification":"supplied CDS engineering example; independent market/model and host certificates pending","certified":false})
        )?
    );
    Ok(())
}
