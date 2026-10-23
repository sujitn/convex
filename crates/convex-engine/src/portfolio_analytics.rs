//! Portfolio analytics aggregation.
//!
//! Aggregates position-level bond analytics into portfolio-level risk metrics.

use std::collections::HashMap;

use convex_core::{
    contracts::{ExactDecimal, FactorTenor},
    Date,
};
use convex_math::decimal::BoundedDecimal;
use rayon::prelude::*;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, HashSet};

use crate::ports::output::{BondQuoteOutput, PortfolioAnalyticsOutput};
use convex_core::ids::{InstrumentId, PortfolioId};
use convex_core::Currency;

use crate::error::EngineError;

/// Portfolio position.
#[derive(Debug, Clone)]
pub struct Position {
    /// Instrument identifier
    pub instrument_id: InstrumentId,
    /// Notional/face value
    pub notional: Decimal,
    /// Sector classification
    pub sector: Option<String>,
    /// Credit rating
    pub rating: Option<String>,
}

/// Portfolio definition.
#[derive(Debug, Clone)]
pub struct Portfolio {
    /// Portfolio identifier
    pub portfolio_id: PortfolioId,
    /// Portfolio name
    pub name: String,
    /// Reporting currency
    pub currency: Currency,
    /// Positions
    pub positions: Vec<Position>,
}

/// Checked supplied-quote portfolio aggregation. Every requested position is retained.
/// Foreign quotes require a richer FX-bound route and are rejected here.
#[derive(Default)]
pub struct PortfolioAnalyzer;

struct Admitted<'a> {
    position: &'a Position,
    quote: &'a BondQuoteOutput,
    value: BoundedDecimal,
}
struct QuoteBook<'a> {
    rows: Vec<Admitted<'a>>,
    total: BoundedDecimal,
    date: Date,
    timestamp: i64,
}
fn failure(id: &str, field: &str) -> EngineError {
    EngineError::PricingError(format!("portfolio position {id}: {field}"))
}
fn arithmetic(error: convex_math::MathError) -> EngineError {
    EngineError::PricingError(error.to_string())
}
fn required(value: Option<Decimal>, id: &str, field: &str) -> Result<Decimal, EngineError> {
    value.ok_or_else(|| failure(id, field))
}
/// Read one complete runtime valuation as an analyzer quote. The envelope carries neither
/// settlement nor currency: the caller supplies the runtime plan's settlement date and the
/// priced instrument version's currency. A measure the result lacks stays absent, so the
/// analyzer refuses instead of defaulting; a measure in an unexpected unit is refused here.
pub fn runtime_bond_quote(
    published: &crate::ports::runtime::PublishedValuation,
    settlement_date: Date,
    currency: Currency,
) -> Result<BondQuoteOutput, EngineError> {
    use convex_core::contracts::{DataQuality, MeasureValue, ResultStatus, ValuationSubject};
    let envelope = &published.result;
    let provenance = envelope.provenance();
    let ValuationSubject::InstrumentVersion { instrument_id, .. } = &provenance.subject else {
        return Err(failure("runtime", "instrument valuation required"));
    };
    let id = instrument_id.as_str();
    if envelope.status() != ResultStatus::Complete {
        return Err(failure(id, "complete runtime result required"));
    }
    let model = |value: f64| {
        Decimal::from_f64_retain(value)
            .map(Some)
            .ok_or_else(|| failure(id, "unrepresentable runtime measure"))
    };
    let read = |name: &str| -> Result<Option<Decimal>, EngineError> {
        let Some((_, result)) = envelope.results().iter().find(|(m, _)| m.as_str() == name) else {
            return Ok(None);
        };
        match (name, &result.value) {
            ("clean_price_mid", MeasureValue::Price(p)) => Ok(Some(p.value().decimal())),
            ("accrued_interest" | "dv01_mid", MeasureValue::ModelPrice(p)) => {
                model(p.value().get())
            }
            ("ytm_mid", MeasureValue::ModelRate(p)) => model(p.value().get()),
            ("modified_duration_mid", MeasureValue::ModelYears(p)) => model(p.value().get()),
            ("convexity_mid", MeasureValue::ModelYearsSquared(p)) => model(p.value().get()),
            ("z_spread_mid", MeasureValue::ModelBps(p)) => model(p.value().get()),
            _ => Err(failure(id, "runtime measure has an unexpected unit")),
        }
    };
    let clean = read("clean_price_mid")?;
    let ytm = read("ytm_mid")?;
    let z_spread = read("z_spread_mid")?;
    Ok(BondQuoteOutput {
        instrument_id: InstrumentId::new(id),
        isin: None,
        currency,
        settlement_date,
        clean_price_bid: None,
        clean_price_mid: clean,
        clean_price_ask: None,
        accrued_interest: read("accrued_interest")?,
        ytm_bid: None,
        ytm_mid: ytm,
        ytm_ask: None,
        ytw: None,
        ytc: None,
        z_spread_bid: None,
        z_spread_mid: z_spread,
        z_spread_ask: None,
        i_spread_bid: None,
        i_spread_mid: None,
        i_spread_ask: None,
        g_spread_bid: None,
        g_spread_mid: None,
        g_spread_ask: None,
        asw_bid: None,
        asw_mid: None,
        asw_ask: None,
        oas_bid: None,
        oas_mid: None,
        oas_ask: None,
        discount_margin_bid: None,
        discount_margin_mid: None,
        discount_margin_ask: None,
        simple_margin_bid: None,
        simple_margin_mid: None,
        simple_margin_ask: None,
        modified_duration: read("modified_duration_mid")?,
        macaulay_duration: None,
        effective_duration: None,
        spread_duration: None,
        convexity: read("convexity_mid")?,
        effective_convexity: None,
        dv01: read("dv01_mid")?,
        pv01: None,
        key_rate_durations: None,
        cs01: None,
        zero_pv01_signed: None,
        z_pv01_signed: None,
        measure_errors: Vec::new(),
        timestamp: provenance.calculated_at.datetime().timestamp_millis(),
        pricing_spec: provenance.model_version.as_str().into(),
        source: published.runtime_id.as_str().into(),
        is_stale: envelope.quality() == DataQuality::Stale,
        // The router's convention: 100 when the mid quote has no measure errors.
        quality: 100,
    })
}
fn ratio(value: &BoundedDecimal, total: &BoundedDecimal) -> Result<Decimal, EngineError> {
    if total.is_zero() {
        return Err(failure(
            "book",
            "weighted measure is undefined at zero total value",
        ));
    }
    let sign = if total.is_negative() {
        -Decimal::ONE
    } else {
        Decimal::ONE
    };
    value
        .multiply(sign)
        .and_then(|n| total.multiply(sign).and_then(|d| n.ratio(&d)))
        .map_err(arithmetic)
}
fn identity(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 8192
}
impl<'a> QuoteBook<'a> {
    fn admit(portfolio: &'a Portfolio, quotes: &'a [BondQuoteOutput]) -> Result<Self, EngineError> {
        if !identity(portfolio.portfolio_id.as_str())
            || portfolio.positions.is_empty()
            || portfolio.positions.len() > 65_536
            || quotes.len() > 65_536
        {
            return Err(failure(
                "book",
                "bounded nonempty portfolio and supplied quote manifests required",
            ));
        }
        let mut wanted = HashSet::new();
        for p in &portfolio.positions {
            if !identity(p.instrument_id.as_str()) || !wanted.insert(&p.instrument_id) {
                return Err(failure(
                    p.instrument_id.as_str(),
                    "nonempty unique instrument position required",
                ));
            }
            for label in [&p.sector, &p.rating].into_iter().flatten() {
                if !identity(label) {
                    return Err(failure(
                        p.instrument_id.as_str(),
                        "bounded nonempty classification required",
                    ));
                }
            }
        }
        let mut lookup = HashMap::new();
        for quote in quotes {
            if wanted.contains(&quote.instrument_id)
                && lookup.insert(&quote.instrument_id, quote).is_some()
            {
                return Err(failure(
                    quote.instrument_id.as_str(),
                    "duplicate consumed quote",
                ));
            }
        }
        let mut rows = Vec::with_capacity(portfolio.positions.len());
        let mut total = BoundedDecimal::zero();
        let mut date = None;
        let mut timestamp = None;
        for position in &portfolio.positions {
            let id = position.instrument_id.as_str();
            let quote = *lookup
                .get(&position.instrument_id)
                .ok_or_else(|| failure(id, "required quote is absent"))?;
            if quote.currency != portfolio.currency {
                return Err(failure(id, "foreign quote requires explicitly bound FX"));
            }
            if date.is_some_and(|d| d != quote.settlement_date) {
                return Err(failure(id, "mixed settlement dates"));
            }
            if quote.is_stale
                || quote.timestamp < 0
                || quote.quality > 100
                || !identity(&quote.source)
                || !identity(&quote.pricing_spec)
            {
                return Err(failure(
                    id,
                    "valid supplied quote lineage and non-stale status required",
                ));
            }
            date = Some(quote.settlement_date);
            timestamp = Some(timestamp.map_or(quote.timestamp, |t: i64| t.max(quote.timestamp)));
            let clean = required(quote.clean_price_mid, id, "clean mid price is absent")?;
            let accrued = required(quote.accrued_interest, id, "accrued per100 is absent")?;
            if clean < Decimal::ZERO {
                return Err(failure(id, "clean price cannot be negative"));
            }
            let mut dirty = BoundedDecimal::decimal(clean);
            dirty
                .add(&BoundedDecimal::decimal(accrued))
                .map_err(arithmetic)?;
            if dirty.is_negative() {
                return Err(failure(id, "dirty price cannot be negative"));
            }
            let value = dirty
                .multiply(position.notional)
                .and_then(|v| v.divide_power_of_ten(2))
                .map_err(arithmetic)?;
            total.add(&value).map_err(arithmetic)?;
            rows.push(Admitted {
                position,
                quote,
                value,
            });
        }
        Ok(Self {
            rows,
            total,
            date: date.ok_or_else(|| failure("book", "settlement date absent"))?,
            timestamp: timestamp.ok_or_else(|| failure("book", "quote timestamp absent"))?,
        })
    }
    fn weighted(
        &self,
        field: &str,
        get: impl Fn(&BondQuoteOutput) -> Option<Decimal>,
    ) -> Result<Decimal, EngineError> {
        let mut sum = BoundedDecimal::zero();
        for row in &self.rows {
            let value = required(get(row.quote), row.position.instrument_id.as_str(), field)?;
            sum.add(&row.value.multiply(value).map_err(arithmetic)?)
                .map_err(arithmetic)?;
        }
        ratio(&sum, &self.total)
    }
    fn classification(
        &self,
        get: impl Fn(&Position) -> &Option<String>,
    ) -> Result<(Vec<(String, ExactDecimal)>, u32), EngineError> {
        let mut sums = BTreeMap::<String, BoundedDecimal>::new();
        let mut covered = 0;
        for row in &self.rows {
            if let Some(label) = get(row.position) {
                sums.entry(label.clone())
                    .or_insert_with(BoundedDecimal::zero)
                    .add(&row.value)
                    .map_err(arithmetic)?;
                covered += 1;
            }
        }
        let values = sums
            .into_iter()
            .map(|(key, value)| ratio(&value, &self.total).map(|v| (key, ExactDecimal::new(v))))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((values, covered))
    }
}
impl PortfolioAnalyzer {
    /// Construct a complete supplied-quote analyzer without partial-coverage defaults.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
    /// Aggregate complete supplied quotes. Unused quotes in the shared pool are not consumed.
    pub fn calculate(
        &self,
        portfolio: &Portfolio,
        bond_prices: &[BondQuoteOutput],
    ) -> Result<PortfolioAnalyticsOutput, EngineError> {
        let book = QuoteBook::admit(portfolio, bond_prices)?;
        let duration = book.weighted("modified duration is absent", |q| q.modified_duration)?;
        let convexity = book.weighted("convexity is absent", |q| q.convexity)?;
        let yield_value = book.weighted("YTM is absent", |q| q.ytm_mid)?;
        let spread = book.weighted("Z-spread is absent", |q| q.z_spread_mid)?;
        let mut dv01 = BoundedDecimal::zero();
        let mut factors = BTreeMap::<String, (BoundedDecimal, u32)>::new();
        let mut work = 0usize;
        for row in &book.rows {
            let id = row.position.instrument_id.as_str();
            let supplied = required(row.quote.dv01, id, "DV01 is absent")?;
            let term = BoundedDecimal::decimal(supplied)
                .multiply(row.position.notional)
                .and_then(|v| v.divide_power_of_ten(2))
                .map_err(arithmetic)?;
            dv01.add(&term).map_err(arithmetic)?;
            if let Some(points) = &row.quote.key_rate_durations {
                work = work
                    .checked_add(points.len())
                    .ok_or_else(|| failure(id, "factor work bound exceeded"))?;
                if points.len() > 1024 || work > 1_048_576 {
                    return Err(failure(id, "factor work bound exceeded"));
                }
                let mut seen = HashSet::new();
                for (tenor, value) in points {
                    let exact =
                        FactorTenor::new(tenor.clone()).map_err(|e| failure(id, &e.to_string()))?;
                    if matches!(exact.as_str(), "spot" | "parallel") || !seen.insert(tenor.as_str())
                    {
                        return Err(failure(id, "distinct dated key-rate factors required"));
                    }
                    let state = factors
                        .entry(tenor.clone())
                        .or_insert_with(|| (BoundedDecimal::zero(), 0));
                    state
                        .0
                        .add(&row.value.multiply(*value).map_err(arithmetic)?)
                        .map_err(arithmetic)?;
                    state.1 += 1;
                }
            }
        }
        if factors.len() > 1024 {
            return Err(failure("book", "requested factor union bound exceeded"));
        }
        let count = book.rows.len() as u32;
        let mut key_rate_durations = Vec::with_capacity(factors.len());
        let mut key_rate_coverage = Vec::with_capacity(factors.len());
        let mut factors = factors.into_iter().collect::<Vec<_>>();
        // Presentation order only; exact labels remain separate identities.
        factors.sort_by_key(|(label, _)| {
            let split = label.len() - 1;
            let number = label[..split].parse::<u32>().ok();
            (label.as_bytes()[split], number, label.clone())
        });
        for (label, (value, coverage)) in factors {
            let duration = if coverage == count {
                Some(ExactDecimal::new(ratio(&value, &book.total)?))
            } else {
                None
            };
            key_rate_coverage.push((label.clone(), coverage));
            key_rate_durations.push((label, duration));
        }
        let (sector_breakdown, sector_coverage) = book.classification(|p| &p.sector)?;
        let (rating_breakdown, rating_coverage) = book.classification(|p| &p.rating)?;
        Ok(PortfolioAnalyticsOutput {
            portfolio_id: portfolio.portfolio_id.clone(),
            name: portfolio.name.clone(),
            currency: portfolio.currency,
            market_value: book.total.finish().map_err(arithmetic)?,
            num_positions: count,
            duration,
            convexity,
            yield_value,
            spread,
            dv01: dv01.finish().map_err(arithmetic)?,
            key_rate_durations,
            key_rate_coverage,
            sector_breakdown,
            sector_coverage,
            rating_breakdown,
            rating_coverage,
            settlement_date: book.date,
            timestamp: book.timestamp,
        })
    }
    /// Preserve one checked outcome per requested portfolio, in request order.
    pub fn calculate_batch(
        &self,
        portfolios: &[Portfolio],
        bond_prices: &[BondQuoteOutput],
    ) -> Vec<Result<PortfolioAnalyticsOutput, EngineError>> {
        portfolios
            .par_iter()
            .map(|p| self.calculate(p, bond_prices))
            .collect()
    }
    /// Complete signed contributions; missing requested duration is an error.
    pub fn duration_contribution(
        &self,
        portfolio: &Portfolio,
        bond_prices: &[BondQuoteOutput],
    ) -> Result<Vec<(InstrumentId, Decimal, Decimal)>, EngineError> {
        let book = QuoteBook::admit(portfolio, bond_prices)?;
        book.rows
            .iter()
            .map(|row| {
                let duration = required(
                    row.quote.modified_duration,
                    row.position.instrument_id.as_str(),
                    "modified duration is absent",
                )?;
                let weight = ratio(&row.value, &book.total)?;
                let contribution = ratio(
                    &row.value.multiply(duration).map_err(arithmetic)?,
                    &book.total,
                )?;
                Ok((row.position.instrument_id.clone(), weight, contribution))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::Date;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_portfolio() -> Portfolio {
        Portfolio {
            portfolio_id: PortfolioId::new("TEST_PORT"),
            name: "Test Portfolio".to_string(),
            currency: Currency::USD,
            positions: vec![
                Position {
                    instrument_id: InstrumentId::new("BOND_A"),
                    notional: dec!(1000000),
                    sector: Some("Technology".to_string()),
                    rating: Some("A".to_string()),
                },
                Position {
                    instrument_id: InstrumentId::new("BOND_B"),
                    notional: dec!(2000000),
                    sector: Some("Healthcare".to_string()),
                    rating: Some("BBB".to_string()),
                },
                Position {
                    instrument_id: InstrumentId::new("BOND_C"),
                    notional: dec!(500000),
                    sector: Some("Technology".to_string()),
                    rating: Some("A".to_string()),
                },
            ],
        }
    }

    fn create_test_quotes() -> Vec<BondQuoteOutput> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        vec![
            BondQuoteOutput {
                instrument_id: InstrumentId::new("BOND_A"),
                isin: None,
                currency: Currency::USD,
                settlement_date: date(2025, 6, 17),
                clean_price_bid: None,
                clean_price_mid: Some(dec!(99.00)),
                clean_price_ask: None,
                accrued_interest: Some(dec!(1.00)),
                ytm_bid: None,
                ytm_mid: Some(dec!(0.05)),
                ytm_ask: None,
                ytw: None,
                ytc: None,
                z_spread_bid: None,
                z_spread_mid: Some(dec!(100)),
                z_spread_ask: None,
                i_spread_bid: None,
                i_spread_mid: None,
                i_spread_ask: None,
                g_spread_bid: None,
                g_spread_mid: None,
                g_spread_ask: None,
                asw_bid: None,
                asw_mid: None,
                asw_ask: None,
                oas_bid: None,
                oas_mid: None,
                oas_ask: None,
                discount_margin_bid: None,
                discount_margin_mid: None,
                discount_margin_ask: None,
                simple_margin_bid: None,
                simple_margin_mid: None,
                simple_margin_ask: None,
                modified_duration: Some(dec!(5.0)),
                macaulay_duration: Some(dec!(5.2)),
                effective_duration: None,
                spread_duration: None,
                convexity: Some(dec!(30)),
                effective_convexity: None,
                dv01: Some(dec!(0.05)),
                pv01: None,
                key_rate_durations: Some(vec![
                    ("2Y".to_string(), dec!(1.0)),
                    ("5Y".to_string(), dec!(3.0)),
                    ("10Y".to_string(), dec!(1.0)),
                ]),
                cs01: None,
                timestamp: now,
                pricing_spec: "test".to_string(),
                source: "test".to_string(),
                is_stale: false,
                zero_pv01_signed: None,
                z_pv01_signed: None,
                measure_errors: Vec::new(),
                quality: 100,
            },
            BondQuoteOutput {
                instrument_id: InstrumentId::new("BOND_B"),
                isin: None,
                currency: Currency::USD,
                settlement_date: date(2025, 6, 17),
                clean_price_bid: None,
                clean_price_mid: Some(dec!(101.00)),
                clean_price_ask: None,
                accrued_interest: Some(dec!(1.00)),
                ytm_bid: None,
                ytm_mid: Some(dec!(0.045)),
                ytm_ask: None,
                ytw: None,
                ytc: None,
                z_spread_bid: None,
                z_spread_mid: Some(dec!(80)),
                z_spread_ask: None,
                i_spread_bid: None,
                i_spread_mid: None,
                i_spread_ask: None,
                g_spread_bid: None,
                g_spread_mid: None,
                g_spread_ask: None,
                asw_bid: None,
                asw_mid: None,
                asw_ask: None,
                oas_bid: None,
                oas_mid: None,
                oas_ask: None,
                discount_margin_bid: None,
                discount_margin_mid: None,
                discount_margin_ask: None,
                simple_margin_bid: None,
                simple_margin_mid: None,
                simple_margin_ask: None,
                modified_duration: Some(dec!(7.0)),
                macaulay_duration: Some(dec!(7.3)),
                effective_duration: None,
                spread_duration: None,
                convexity: Some(dec!(55)),
                effective_convexity: None,
                dv01: Some(dec!(0.071)),
                pv01: None,
                key_rate_durations: Some(vec![
                    ("2Y".to_string(), dec!(0.5)),
                    ("5Y".to_string(), dec!(2.0)),
                    ("10Y".to_string(), dec!(4.5)),
                ]),
                cs01: None,
                timestamp: now,
                pricing_spec: "test".to_string(),
                source: "test".to_string(),
                is_stale: false,
                zero_pv01_signed: None,
                z_pv01_signed: None,
                measure_errors: Vec::new(),
                quality: 100,
            },
            BondQuoteOutput {
                instrument_id: InstrumentId::new("BOND_C"),
                isin: None,
                currency: Currency::USD,
                settlement_date: date(2025, 6, 17),
                clean_price_bid: None,
                clean_price_mid: Some(dec!(98.00)),
                clean_price_ask: None,
                accrued_interest: Some(dec!(1.00)),
                ytm_bid: None,
                ytm_mid: Some(dec!(0.055)),
                ytm_ask: None,
                ytw: None,
                ytc: None,
                z_spread_bid: None,
                z_spread_mid: Some(dec!(120)),
                z_spread_ask: None,
                i_spread_bid: None,
                i_spread_mid: None,
                i_spread_ask: None,
                g_spread_bid: None,
                g_spread_mid: None,
                g_spread_ask: None,
                asw_bid: None,
                asw_mid: None,
                asw_ask: None,
                oas_bid: None,
                oas_mid: None,
                oas_ask: None,
                discount_margin_bid: None,
                discount_margin_mid: None,
                discount_margin_ask: None,
                simple_margin_bid: None,
                simple_margin_mid: None,
                simple_margin_ask: None,
                modified_duration: Some(dec!(3.0)),
                macaulay_duration: Some(dec!(3.1)),
                effective_duration: None,
                spread_duration: None,
                convexity: Some(dec!(12)),
                effective_convexity: None,
                dv01: Some(dec!(0.03)),
                pv01: None,
                key_rate_durations: Some(vec![
                    ("2Y".to_string(), dec!(2.0)),
                    ("5Y".to_string(), dec!(1.0)),
                ]),
                cs01: None,
                timestamp: now,
                pricing_spec: "test".to_string(),
                source: "test".to_string(),
                is_stale: false,
                zero_pv01_signed: None,
                z_pv01_signed: None,
                measure_errors: Vec::new(),
                quality: 100,
            },
        ]
    }

    #[test]
    fn test_portfolio_analytics() {
        let analyzer = PortfolioAnalyzer::new();
        let portfolio = create_test_portfolio();
        let quotes = create_test_quotes();

        let result = analyzer.calculate(&portfolio, &quotes);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.portfolio_id, PortfolioId::new("TEST_PORT"));
        assert_eq!(output.num_positions, 3);

        // Check market value: 1M * 100% + 2M * 102% + 0.5M * 99%
        // = 1,000,000 + 2,040,000 + 495,000 = 3,535,000
        let mv = output.market_value.to_f64().unwrap();
        assert!(
            (mv - 3_535_000.0).abs() < 1.0,
            "Market value should be ~3.535M: {}",
            mv
        );

        // Duration should be weighted average
        let dur = output.duration.to_f64().unwrap();
        assert!(
            dur > 0.0 && dur < 10.0,
            "Duration should be reasonable: {}",
            dur
        );

        // Check sector breakdown sums to ~1.0
        let sector_sum: Decimal = output
            .sector_breakdown
            .iter()
            .map(|(_, w)| w.decimal())
            .sum();
        assert!(
            (sector_sum.to_f64().unwrap() - 1.0).abs() < 0.01,
            "Sector weights should sum to 1: {}",
            sector_sum
        );
    }

    #[test]
    fn test_duration_contribution() {
        let analyzer = PortfolioAnalyzer::new();
        let portfolio = create_test_portfolio();
        let quotes = create_test_quotes();

        let contributions = analyzer.duration_contribution(&portfolio, &quotes).unwrap();
        assert_eq!(contributions.len(), 3);

        // Contributions should sum to portfolio duration
        let total_contribution: Decimal = contributions.iter().map(|(_, _, c)| *c).sum();

        // Get portfolio duration for comparison
        let output = analyzer.calculate(&portfolio, &quotes).unwrap();
        let port_dur = output.duration;

        let diff = (total_contribution - port_dur).abs();
        assert!(
            diff < dec!(0.01),
            "Contributions should sum to portfolio duration: {} vs {}",
            total_contribution,
            port_dur
        );
    }

    #[test]
    fn test_key_rate_duration_aggregation() {
        let analyzer = PortfolioAnalyzer::new();
        let portfolio = create_test_portfolio();
        let quotes = create_test_quotes();

        let result = analyzer.calculate(&portfolio, &quotes).unwrap();

        // Should have key rate durations
        assert!(!result.key_rate_durations.is_empty());

        // Check that KRDs are sorted by tenor
        let tenors: Vec<&str> = result
            .key_rate_durations
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(tenors, vec!["2Y", "5Y", "10Y"]);
    }
}
