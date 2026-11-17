//! The level stage (P19a; D111–D113, D112 as amended): a stored fixed bullet quoted by a
//! yield or a spread is priced from its level, and the bond's model then values that
//! price as it values any other.
//!
//! The stage adds no formula. It selects the mark a rule names and the benchmark
//! securities a spread is quoted to, and the price comes from
//! `convex_analytics::bond_quote::price_from_level`, the definitions the router publishes,
//! so a yield or Z-spread priced here is the one the router reports back. A benchmark
//! spread is over the one security its tenor names; the router's G-spread is over the
//! whole set. A benchmark security's yield is its street yield at its Live clean price
//! mid, read from the Live runtime's record of it: quotes are not stored (D5).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_analytics::{
    bond_quote::{government_curve, price_from_level, BondCurves},
    dto::GovernmentCurveSpec,
    spreads::{GovernmentBenchmark, GovernmentCurve, SecurityId, Sovereign},
};
use convex_bonds::prelude::FixedRateBond;
use convex_bonds::types::Tenor;
use convex_core::{
    contracts::{
        InstrumentId, MeasureValue, ReferenceDataId, SnapshotId, UtcTimestamp, ValuationSubject,
        Version,
    },
    types::{Compounding, Mark, Spread, SpreadType, Yield},
    Currency, Date,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::ops::RangeInclusive;

use crate::curve_builder::BuiltCurve;
use crate::ports::{
    level_pricing::{
        is_plain_bullet, tenor_months, BenchmarkSecurity, PricingMethod, PricingRuleDefinition,
    },
    reference_data::BondReferenceData,
    runtime::PublishedValuation,
};
use crate::pricing_router::PricingRouter;
use crate::runtime::accepted;

/// The stage's model id, recorded beside a level-priced result.
pub const LEVEL_PRICE_MODEL: &str = "convex-level-price-v1";
/// A yield level is a decimal from -10% to 100%. A yield sent in percent (6.12 for
/// 6.12%) is outside it and refused, not priced at 612%.
const YIELD_LEVEL_RANGE: RangeInclusive<Decimal> = dec!(-0.1)..=dec!(1);
/// A spread level is from -2,000 to 20,000 basis points.
const SPREAD_LEVEL_RANGE_BPS: RangeInclusive<Decimal> = dec!(-2000)..=dec!(20000);

/// Why a level was not priced. Nothing is substituted.
#[derive(Debug, thiserror::Error)]
pub enum LevelError {
    /// The request or the stored rule is not one the stage prices.
    #[error("{0}")]
    Refused(String),
    /// A benchmark security has no usable Live price. The message names it by its tenor
    /// in the set, not its instrument: a caller may be entitled to the bond and not to
    /// the benchmarks, and a refusal is not checked against entitlements.
    #[error("the {tenor} benchmark has no usable Live price: {reason}")]
    BenchmarkUnavailable {
        /// The security's tenor in the set.
        tenor: String,
        /// The security's instrument, for the engine's callers and logs; not in the
        /// message.
        instrument_id: InstrumentId,
        /// Why its Live record cannot be read.
        reason: &'static str,
    },
    /// The pure crates could not price the level.
    #[error("the level could not be priced: {0}")]
    Analytics(String),
    /// A rule, set or security could not be read.
    #[error(transparent)]
    Reference(#[from] crate::ports::reference_repository::ReferenceError),
}

/// One security of a benchmark set as the stage read it.
#[derive(Debug, Clone)]
pub struct BenchmarkQuote {
    /// The set's entry: its tenor, instrument and pinned version.
    pub security: BenchmarkSecurity,
    /// Its terms at that version.
    pub bond: BondReferenceData,
    /// Its clean price mid in the Live record read.
    pub clean_price_mid: Decimal,
    /// The Live snapshot that record valued.
    pub snapshot_id: SnapshotId,
}

/// A benchmark set in force and each of its securities as read, in the set's order.
#[derive(Debug, Clone)]
pub struct BenchmarkSetQuotes {
    /// The set's id.
    pub benchmark_set_id: ReferenceDataId,
    /// Its version in force.
    pub version: Version,
    /// Each security, in the set's order.
    pub quotes: Vec<BenchmarkQuote>,
}

/// A bond's pricing rule in force and, when it names one, its benchmark set as read, as
/// of one cutoff.
#[derive(Debug, Clone)]
pub struct PricingRuleInForce {
    /// The rule.
    pub rule: PricingRuleDefinition,
    /// Its version in force.
    pub rule_version: Version,
    /// The set the rule names, as read.
    pub benchmark_set: Option<BenchmarkSetQuotes>,
}

/// A security's clean price mid in its Live record, and the snapshot that record valued.
#[derive(Debug, Clone)]
pub struct LivePrice {
    /// The clean price mid.
    pub clean_price_mid: Decimal,
    /// The Live snapshot valued.
    pub snapshot_id: SnapshotId,
}

/// The clean price mid of a benchmark security in its Live record, read at `known_at`.
/// The record must value the version the set pins, be no newer than the cutoff, still be
/// live, be of a quality the books accept from Live results (`Valid`, or `Demo` when the
/// configuration accepts demonstration data), and carry a clean price: a partial result
/// carrying one is used, and a failed result carries none.
pub fn live_clean_price_mid(
    record: Option<&PublishedValuation>,
    security: &BenchmarkSecurity,
    known_at: UtcTimestamp,
    accepts_demo: bool,
) -> Result<LivePrice, LevelError> {
    let unavailable = |reason| benchmark_unavailable(security, reason);
    let record = record.ok_or_else(|| unavailable("the Live runtime does not value it"))?;
    let envelope = &record.result;
    let provenance = envelope.provenance();
    match &provenance.subject {
        ValuationSubject::InstrumentVersion {
            instrument_id,
            version,
        } if instrument_id == &security.instrument_id && *version == security.version => {}
        _ => return Err(unavailable("the Live runtime values another version of it")),
    }
    if provenance.known_at > known_at {
        return Err(unavailable(
            "its Live price is newer than the cutoff, and Live prices are not stored",
        ));
    }
    if !accepted(envelope.quality(), accepts_demo)
        || record
            .live_valid_until
            .is_some_and(|until| until <= known_at)
    {
        return Err(unavailable("its Live price is stale or not accepted"));
    }
    let clean_price_mid = envelope
        .results()
        .iter()
        .find(|(measure, _)| measure.as_str() == "clean_price_mid")
        .and_then(|(_, result)| match &result.value {
            MeasureValue::Price(price) => Some(price.value().decimal()),
            _ => None,
        })
        .ok_or_else(|| unavailable("its Live record has no clean price"))?;
    Ok(LivePrice {
        clean_price_mid,
        snapshot_id: envelope.snapshot_id().clone(),
    })
}

/// What a request quotes: a clean price, or a level in the rule's unit (a decimal yield,
/// or basis points of spread).
#[derive(Debug, Clone, Copy)]
pub enum QuotedInput {
    /// A clean price per 100 face.
    Price(Decimal),
    /// A level in the rule's unit.
    Level(Decimal),
}

/// The yield of the benchmark a spread was quoted to: the yield the spread is added to.
#[derive(Debug, Clone)]
pub struct BenchmarkYield {
    /// Its tenor, as the set labels it.
    pub tenor: String,
    /// Its instrument.
    pub instrument_id: InstrumentId,
    /// Its yield to maturity at the request's settlement, compounded semi-annually, as
    /// a decimal: its street yield for a semi-annual Treasury, converted for a security
    /// paying at another frequency.
    pub ytm: f64,
}

/// The clean price a quoted input gives under the bond's rule, and what the bond's model
/// reads beside it.
pub struct RulePricing {
    /// The clean price mid the model values.
    pub clean_price_mid: Decimal,
    /// Whether the configuration's percent-of-price bid/ask width applies: not to a
    /// level-quoted bond, whose width is in its quoting unit (D114).
    pub price_width_applies: bool,
    /// The set's securities, for the G-spread, when the rule names a set and the bond
    /// matures within the set's maturities.
    pub government_curve: Option<GovernmentCurve>,
    /// Why no G-spread is published although the rule names a set.
    pub g_spread_refused: Option<&'static str>,
    /// The benchmark a spread was quoted to.
    pub benchmark: Option<BenchmarkYield>,
}

/// Price `input` for `bond` under its rule, if it has one. With no rule the bond is quoted
/// by price, and a price is taken as given; under a rule a level is priced through
/// `price_from_level`. `discount` is the configuration's discount curve, which a Z-spread
/// reads. A level or a set on anything but a fixed bullet is refused.
pub fn price_under_rule(
    bond: &BondReferenceData,
    in_force: Option<&PricingRuleInForce>,
    input: QuotedInput,
    settlement: Date,
    discount: Option<&BuiltCurve>,
) -> Result<RulePricing, LevelError> {
    let Some(in_force) = in_force else {
        return match input {
            QuotedInput::Price(price) => Ok(RulePricing {
                clean_price_mid: price,
                price_width_applies: true,
                government_curve: None,
                g_spread_refused: None,
                benchmark: None,
            }),
            QuotedInput::Level(_) => Err(refused(
                "this bond has no pricing rule, so it is quoted by price; send market_price",
            )),
        };
    };
    let rule = &in_force.rule;
    if rule.instrument_id.as_str() != bond.instrument_id.as_str() {
        return Err(refused("the pricing rule is for another instrument"));
    }
    if rule.requires_plain_bullet() && !is_plain_bullet(bond) {
        return Err(refused(
            "only a fixed bullet is priced from a level or against a benchmark set",
        ));
    }
    let benchmarks = match &in_force.benchmark_set {
        Some(set) => Some(price_benchmarks(&set.quotes, settlement)?),
        None => None,
    };
    let (clean_price_mid, benchmark) = match input {
        QuotedInput::Price(price) => (price, None),
        QuotedInput::Level(level) => level_to_clean_price(
            bond,
            &rule.method,
            level,
            settlement,
            discount,
            benchmarks.as_deref(),
        )?,
    };
    let (government_curve, g_spread_refused) = match benchmarks {
        Some(all) if matures_within(bond.maturity_date, &all) => (
            Some(benchmark_curve(
                all.into_iter().map(|b| b.government_benchmark).collect(),
                settlement,
                bond.currency,
            )?),
            None,
        ),
        Some(_) => (
            None,
            Some("the bond matures outside its benchmark set's maturities"),
        ),
        None => (None, None),
    };
    Ok(RulePricing {
        clean_price_mid,
        price_width_applies: !rule.method.is_level(),
        government_curve,
        g_spread_refused,
        benchmark,
    })
}

/// The clean price a level gives under `method`, and the benchmark a spread named.
fn level_to_clean_price(
    bond: &BondReferenceData,
    method: &PricingMethod,
    level: Decimal,
    settlement: Date,
    discount: Option<&BuiltCurve>,
    benchmarks: Option<&[PricedBenchmark<'_>]>,
) -> Result<(Decimal, Option<BenchmarkYield>), LevelError> {
    // The level is checked before the bond is built.
    let (range, unit) = match method {
        PricingMethod::Price {} => return Err(quoted_by_price()),
        PricingMethod::Yield {} => (YIELD_LEVEL_RANGE, "a decimal yield"),
        PricingMethod::ZSpread {} | PricingMethod::BenchmarkSpread { .. } => {
            (SPREAD_LEVEL_RANGE_BPS, "basis points of spread")
        }
    };
    if !range.contains(&level) {
        return Err(refused(format!(
            "a {} level is {unit} from {} to {}",
            method.name(),
            range.start(),
            range.end()
        )));
    }
    let fixed = fixed_rate_bond(bond)?;
    let frequency = fixed.frequency();
    let spread = |kind| Mark::Spread {
        value: Spread::new(level, kind),
        benchmark: method.name().into(),
    };
    let single_benchmark;
    let (mark, curves, benchmark) = match method {
        PricingMethod::Price {} => return Err(quoted_by_price()),
        PricingMethod::Yield {} => (
            Mark::Yield {
                value: level,
                frequency,
            },
            BondCurves::default(),
            None,
        ),
        PricingMethod::ZSpread {} => {
            let discount = discount
                .ok_or_else(|| refused("a Z-spread needs the configuration's discount curve"))?;
            (
                spread(SpreadType::ZSpread),
                BondCurves {
                    discount: Some(discount),
                    ..Default::default()
                },
                None,
            )
        }
        PricingMethod::BenchmarkSpread { tenor } => {
            // By months, so a rule's `1Y` finds a set's `12M`, as the set's validation
            // treats them as one tenor.
            let months = tenor_months(tenor).map_err(|e| refused(e.to_string()))?;
            let quoted_to = benchmarks
                .ok_or_else(|| refused("a benchmark spread names its benchmark set"))?
                .iter()
                .find(|b| b.months == months)
                .ok_or_else(|| refused(format!("the benchmark set has no {tenor} security")))?;
            single_benchmark = benchmark_curve(
                vec![quoted_to.government_benchmark.clone()],
                settlement,
                bond.currency,
            )?;
            // The yield `price_from_level` adds the spread to, so the one reported is the
            // one applied.
            let ytm = single_benchmark
                .supplied_yield(settlement, bond.maturity_date)
                .map_err(analytics_error)?
                .value()
                .as_f64();
            (
                spread(SpreadType::GSpread),
                BondCurves {
                    government: Some(&single_benchmark),
                    ..Default::default()
                },
                Some(BenchmarkYield {
                    tenor: quoted_to.quote.security.tenor.clone(),
                    instrument_id: quoted_to.quote.security.instrument_id.clone(),
                    ytm,
                }),
            )
        }
    };
    let priced =
        price_from_level(&fixed, settlement, &mark, frequency, &curves).map_err(analytics_error)?;
    let clean = Decimal::from_f64_retain(priced.clean_price_per_100)
        .ok_or_else(|| LevelError::Analytics("the level's price is not finite".into()))?;
    Ok((clean, benchmark))
}

/// The bond as the router builds it.
fn fixed_rate_bond(bond: &BondReferenceData) -> Result<FixedRateBond, LevelError> {
    PricingRouter::new()
        .to_fixed_rate_bond(bond)
        .map_err(analytics_error)
}

fn quoted_by_price() -> LevelError {
    refused("this bond is quoted by price; send market_price, not a level")
}

/// A benchmark security of a set with no usable Live price, and why.
pub(crate) fn benchmark_unavailable(
    security: &BenchmarkSecurity,
    reason: &'static str,
) -> LevelError {
    LevelError::BenchmarkUnavailable {
        tenor: security.tenor.clone(),
        instrument_id: security.instrument_id.clone(),
        reason,
    }
}

fn refused(reason: impl Into<String>) -> LevelError {
    LevelError::Refused(reason.into())
}

fn analytics_error(error: impl std::fmt::Display) -> LevelError {
    LevelError::Analytics(error.to_string())
}

/// A benchmark security priced for one request: its quote, its tenor in months and the
/// pure crate's record of it, which carries its street yield.
struct PricedBenchmark<'a> {
    quote: &'a BenchmarkQuote,
    months: u32,
    government_benchmark: GovernmentBenchmark,
}

/// Each security of a set with its street yield at `settlement`, from its Live clean
/// price mid. Each is a sovereign plain bullet in USD, checked when the set was committed
/// at the version it pins.
fn price_benchmarks(
    quotes: &[BenchmarkQuote],
    settlement: Date,
) -> Result<Vec<PricedBenchmark<'_>>, LevelError> {
    quotes
        .iter()
        .map(|quote| {
            let bond = &quote.bond;
            let fixed = fixed_rate_bond(bond)?;
            let frequency = fixed.frequency();
            let ytm = convex_analytics::functions::yield_to_maturity(
                &fixed,
                settlement,
                quote.clean_price_mid,
                frequency,
            )
            .map_err(analytics_error)?
            .yield_value;
            let rate = Decimal::from_f64_retain(ytm)
                .ok_or_else(|| LevelError::Analytics("a benchmark yield is not finite".into()))?;
            let security_id = bond
                .cusip
                .as_deref()
                .and_then(|c| SecurityId::cusip(c).ok())
                .or_else(|| bond.isin.as_deref().and_then(|i| SecurityId::isin(i).ok()))
                .ok_or_else(|| {
                    refused(format!(
                        "the {} benchmark has no valid CUSIP or ISIN",
                        quote.security.tenor
                    ))
                })?;
            let months = tenor_months(&quote.security.tenor).map_err(|e| refused(e.to_string()))?;
            Ok(PricedBenchmark {
                quote,
                months,
                government_benchmark: GovernmentBenchmark::new(
                    Sovereign::UST,
                    Tenor::Custom(months),
                    security_id,
                    bond.maturity_date,
                    bond.coupon_rate.unwrap_or(Decimal::ZERO),
                    Yield::new(rate, Compounding::from(frequency)),
                )
                .with_price(quote.clean_price_mid),
            })
        })
        .collect()
}

/// The pure crate's validated curve of actual securities.
fn benchmark_curve(
    benchmarks: Vec<GovernmentBenchmark>,
    settlement: Date,
    currency: Currency,
) -> Result<GovernmentCurve, LevelError> {
    let spec = GovernmentCurveSpec {
        reference_date: settlement,
        sovereign: Sovereign::UST,
        benchmarks,
    };
    government_curve(&spec, settlement, currency).map_err(analytics_error)
}

/// Whether `maturity` lies within the set's maturities, so the G-spread interpolates and
/// never extrapolates.
fn matures_within(maturity: Date, benchmarks: &[PricedBenchmark<'_>]) -> bool {
    let maturities = benchmarks.iter().map(|b| b.government_benchmark.maturity);
    match (maturities.clone().min(), maturities.max()) {
        (Some(first), Some(last)) => first <= maturity && maturity <= last,
        _ => false,
    }
}
