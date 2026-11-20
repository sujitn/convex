//! The level stage: a stored fixed bullet quoted by a yield or a spread is priced from
//! its level, and the bond's model then values that price as it values any other.
//!
//! The stage adds no formula. It selects the mark a rule names and the benchmark
//! securities a spread is quoted to, and the price comes from
//! `convex_analytics::bond_quote::price_from_level`, the definitions the router publishes,
//! so a yield or Z-spread priced here is the one the router reports back. A benchmark
//! spread is over the one security its tenor names; the router's G-spread is over the
//! whole set. A benchmark security's yield is its street yield at its clean price mid:
//! in a snapshot, from its quote in that snapshot ([`price_in_snapshot`]); at request
//! time, from the Live runtime's record of it, since quotes are not stored
//! ([`live_clean_price_mid`]). At request time a rule's width widens what it quotes, and
//! each side is priced as the mid is ([`BidAsk`]); Live publishes mid measures only. The
//! calculator prices a level by any method, mid only, against the set the rule names
//! ([`price_by_method`]).
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
        ErrorCode, InstrumentId, MeasureValue, ReferenceDataId, SnapshotId, UtcTimestamp,
        ValuationSubject, Version,
    },
    types::{
        Compounding, Mark, PriceKind, QuoteWidth, QuoteWidthError, Spread, SpreadType,
        WidthSchedule, Yield,
    },
    Currency, Date,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::{Arc, Mutex};

use crate::curve_builder::BuiltCurve;
use crate::ports::{
    level_pricing::{
        is_plain_bullet, is_quoted_two_way, tenor_months, BenchmarkSecurity, PricingMethod,
        ResolvedPricingRule, WIDTH_NEEDS_TWO_WAY,
    },
    quote_event::QuoteField,
    reference_data::BondReferenceData,
    reference_master::ResolvedBondReference,
    runtime::PublishedValuation,
    snapshot::MarketSnapshot,
    storage::BidAskSpreadConfig,
};
use crate::pricing_router::{PricingInput, PricingRouter};
use crate::runtime::accepted;

/// Why a rule that quotes a level or names a benchmark set does not price a bond.
const NOT_A_PLAIN_BULLET: &str =
    "only a fixed bullet is priced from a level or against a benchmark set";
/// The stage's model id, recorded beside a level-priced result.
pub const LEVEL_PRICE_MODEL: &str = "convex-level-price-v1";
/// A yield level is a decimal from -10% to 100%. A yield sent in percent (6.12 for
/// 6.12%) is outside it and refused, not priced at 612%.
const YIELD_LEVEL_RANGE: RangeInclusive<Decimal> = dec!(-0.1)..=dec!(1);
/// A spread level is from -2,000 to 20,000 basis points.
const SPREAD_LEVEL_RANGE_BPS: RangeInclusive<Decimal> = dec!(-2000)..=dec!(20000);
/// Benchmark yields a [`BenchmarkYieldCache`] keeps; it starts again when full.
const MAX_SOLVED_YIELDS: usize = 4_096;

/// Why a level was not priced. Nothing is substituted.
#[derive(Debug, Clone, thiserror::Error)]
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

impl LevelError {
    /// The canonical code a valuation records for this refusal.
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::Refused(_) | Self::Reference(_) => ErrorCode::InvalidInput,
            Self::BenchmarkUnavailable { .. } => ErrorCode::MissingMarketData,
            Self::Analytics(_) => ErrorCode::SolverLimit,
        }
    }
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

/// A bond's pricing rule, as pinned with its reference, its width at that version, and,
/// when it names one, its benchmark set as read at the same cutoff.
#[derive(Debug, Clone)]
pub struct PricingRuleInForce {
    /// The rule's pin: its version, method and set.
    pub rule: ResolvedPricingRule,
    /// The rule's two-way width at the pinned version. It is read at request time and not
    /// pinned, since Live publishes mid measures only, and not read for a route that
    /// quotes mid only.
    pub width: Option<WidthSchedule>,
    /// The set the rule names, as read; shared by every bond of one request quoted to it.
    pub benchmark_set: Option<Arc<BenchmarkSetQuotes>>,
}

/// The benchmark sets one request has read, by id, failures included, so a batch reads
/// each set and its securities' Live prices once. It holds at most one entry for each
/// bond the request prices.
#[derive(Debug, Default)]
pub(crate) struct BenchmarkSetReads(
    HashMap<ReferenceDataId, Result<Arc<BenchmarkSetQuotes>, LevelError>>,
);

impl BenchmarkSetReads {
    /// The set `id` as this request read it, if it has.
    pub(crate) fn get(
        &self,
        id: &ReferenceDataId,
    ) -> Option<Result<Arc<BenchmarkSetQuotes>, LevelError>> {
        self.0.get(id).cloned()
    }

    /// Keep what reading `id` gave, and return it.
    pub(crate) fn keep(
        &mut self,
        id: ReferenceDataId,
        read: Result<BenchmarkSetQuotes, LevelError>,
    ) -> Result<Arc<BenchmarkSetQuotes>, LevelError> {
        let read = read.map(Arc::new);
        self.0.insert(id, read.clone());
        read
    }
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

impl QuotedInput {
    /// `value` as a bond quoted by `method` quotes it: a level, or a price.
    pub fn quoted_by(method: &PricingMethod, value: Decimal) -> Self {
        if method.is_level() {
            Self::Level(value)
        } else {
            Self::Price(value)
        }
    }
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

/// The two-way a quote carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BidAsk {
    /// The configuration's percent-of-price width: a bond quoted by price whose rule, if
    /// it has one, sets no width.
    ConfigurationWidth,
    /// The rule's width, applied to what the rule quotes.
    RuleWidth(TwoWay),
    /// No two-way: a level-quoted bond whose rule sets no width, or one sent a price.
    MidOnly,
}

impl BidAsk {
    /// Give the router's `input` this two-way, and only it: the rule's sides as its bid
    /// and ask prices, `configured` (the configuration's percent-of-price width), or
    /// neither. Any bid, ask or width the input held is replaced.
    pub fn apply(self, input: &mut PricingInput, configured: Option<&BidAskSpreadConfig>) {
        let (bid, ask, width) = match self {
            Self::ConfigurationWidth => (None, None, configured.cloned()),
            Self::RuleWidth(two_way) => (
                Some(two_way.clean_price_bid),
                Some(two_way.clean_price_ask),
                None,
            ),
            Self::MidOnly => (None, None, None),
        };
        input.market_price_bid = bid;
        input.market_price_ask = ask;
        input.bid_ask_config = width;
    }
}

/// A two-way at a rule's width: each side's level in the rule's unit (a clean price, a
/// decimal yield or basis points of spread) and the clean price it gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwoWay {
    /// The width applied: the first size bucket's, since a request carries no trade size.
    pub width: QuoteWidth,
    /// The bid's level.
    pub bid_level: Decimal,
    /// The ask's level.
    pub ask_level: Decimal,
    /// The bid's clean price.
    pub clean_price_bid: Decimal,
    /// The ask's clean price.
    pub clean_price_ask: Decimal,
}

/// The clean price a quoted input gives under the bond's rule, and what the bond's model
/// reads beside it.
pub struct RulePricing {
    /// The clean price mid the model values.
    pub clean_price_mid: Decimal,
    /// The two-way the quote carries.
    pub bid_ask: BidAsk,
    /// The set's securities, for the G-spread, when the rule names a set and the bond
    /// matures within the set's maturities.
    pub government_curve: Option<GovernmentCurve>,
    /// Why no G-spread is published although the rule names a set.
    pub g_spread_refused: Option<&'static str>,
    /// The benchmark a spread was quoted to.
    pub benchmark: Option<BenchmarkYield>,
}

/// Each benchmark security's street yield, solved once for each settlement and clean
/// price mid and shared by every bond quoted to it: a benchmark tick re-solves each of its
/// securities once, however many bonds it reprices. The engine keeps one for request-time
/// quotes, and each runtime's valuator one for its ticks.
/// It holds at most 4,096 yields and starts again when full.
#[derive(Debug, Default)]
pub struct BenchmarkYieldCache {
    solved: Mutex<HashMap<SolvedYieldKey, Yield>>,
}

/// What a benchmark's street yield depends on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SolvedYieldKey {
    instrument_id: InstrumentId,
    version: Version,
    settlement: Date,
    clean_price_mid: Decimal,
}

impl BenchmarkYieldCache {
    /// `benchmark`'s street yield at `settlement`, at its own coupon frequency.
    fn street_yield(
        &self,
        benchmark: &BenchmarkInput<'_>,
        settlement: Date,
    ) -> Result<Yield, LevelError> {
        let key = SolvedYieldKey {
            instrument_id: benchmark.security.instrument_id.clone(),
            version: benchmark.security.version,
            settlement,
            clean_price_mid: benchmark.clean_price_mid,
        };
        // A poisoned lock only loses the shared yields; each is solved again.
        if let Some(solved) = self
            .solved
            .lock()
            .ok()
            .and_then(|solved| solved.get(&key).copied())
        {
            return Ok(solved);
        }
        let fixed = fixed_rate_bond(benchmark.bond)?;
        let frequency = fixed.frequency();
        let ytm = convex_analytics::functions::yield_to_maturity(
            &fixed,
            settlement,
            benchmark.clean_price_mid,
            frequency,
        )
        .map_err(analytics_error)?
        .yield_value;
        let rate = Decimal::from_f64_retain(ytm)
            .ok_or_else(|| LevelError::Analytics("a benchmark yield is not finite".into()))?;
        let solved = Yield::new(rate, Compounding::from(frequency));
        if let Ok(mut yields) = self.solved.lock() {
            if yields.len() >= MAX_SOLVED_YIELDS {
                yields.clear();
            }
            yields.insert(key, solved);
        }
        Ok(solved)
    }
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
    yields: &BenchmarkYieldCache,
) -> Result<RulePricing, LevelError> {
    let Some(in_force) = in_force else {
        return price_without_rule(input);
    };
    let method = &in_force.rule.method;
    let benchmarks = in_force.benchmark_set.as_deref().map(benchmark_inputs);
    let pricing = price_with_rule(
        bond,
        method,
        in_force.width.as_ref(),
        benchmarks.as_deref(),
        input,
        settlement,
        discount,
        yields,
    )?;
    // A bond quoted by price whose rule sets no width takes the configuration's width, as
    // a bond with no rule does.
    if !method.is_level() && in_force.width.is_none() {
        return Ok(RulePricing {
            bid_ask: BidAsk::ConfigurationWidth,
            ..pricing
        });
    }
    Ok(pricing)
}

/// Price `level`, quoted by `method` whatever the bond's rule quotes, for the calculator:
/// mid only, since no width is applied. `set` is the benchmark set the bond's rule names,
/// as read: a benchmark spread is to one of its securities, and the G-spread is over it.
/// With none, every method but a benchmark spread is priced.
pub fn price_by_method(
    bond: &BondReferenceData,
    set: Option<&BenchmarkSetQuotes>,
    method: &PricingMethod,
    level: Decimal,
    settlement: Date,
    discount: Option<&BuiltCurve>,
    yields: &BenchmarkYieldCache,
) -> Result<RulePricing, LevelError> {
    let benchmarks = set.map(benchmark_inputs);
    price_with_rule(
        bond,
        method,
        None,
        benchmarks.as_deref(),
        QuotedInput::quoted_by(method, level),
        settlement,
        discount,
        yields,
    )
}

/// Each security of a set as read, borrowed for the stage.
fn benchmark_inputs(set: &BenchmarkSetQuotes) -> Vec<BenchmarkInput<'_>> {
    set.quotes
        .iter()
        .map(|quote| BenchmarkInput {
            security: &quote.security,
            bond: &quote.bond,
            clean_price_mid: quote.clean_price_mid,
        })
        .collect()
}

/// [`price_under_rule`] for the bond `reference` resolves in `snapshot`, under the rule the
/// snapshot pins for it: `quoted` is the bond's quote in the rule's field, a level or a
/// price as the rule's method quotes it. Each benchmark security's clean price mid is its
/// quote in the same snapshot, so every bond a Live tick values reads one price for each
/// benchmark. A set the snapshot does not hold is refused by its id; a security it does
/// not hold at its pinned version, or holds no mid price for, by its tenor.
pub fn price_in_snapshot(
    snapshot: &MarketSnapshot,
    reference: &ResolvedBondReference,
    quoted: Decimal,
    settlement: Date,
    discount: Option<&BuiltCurve>,
    yields: &BenchmarkYieldCache,
) -> Result<RulePricing, LevelError> {
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or_else(|| refused("the bond's reference is a tombstone"))?;
    let rule = reference
        .pricing_rule
        .as_ref()
        .ok_or_else(|| refused("the bond has no pricing rule"))?;
    let benchmarks = match &rule.benchmark_set_id {
        None => None,
        Some(id) => {
            let securities = snapshot.benchmark_securities(rule).ok_or_else(|| {
                refused(format!(
                    "benchmark set {} is not in force at the cutoff",
                    id.as_str()
                ))
            })?;
            Some(
                securities
                    .iter()
                    .map(|security| snapshot_benchmark(snapshot, security))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
    };
    // Live publishes mid measures only, so no width is read or applied.
    price_with_rule(
        bond,
        &rule.method,
        None,
        benchmarks.as_deref(),
        QuotedInput::quoted_by(&rule.method, quoted),
        settlement,
        discount,
        yields,
    )
}

/// A benchmark security's terms as `snapshot` holds them, at the version its set pins.
fn held_benchmark<'a>(
    snapshot: &'a MarketSnapshot,
    security: &BenchmarkSecurity,
) -> Option<&'a BondReferenceData> {
    snapshot
        .reference(security.instrument_id.as_str())
        .filter(|reference| reference.record.version == security.version)
        .and_then(|reference| reference.record.bond.as_ref())
}

/// A benchmark security as `snapshot` holds it: its terms at the version the set pins,
/// and the mid price of its quote.
fn snapshot_benchmark<'a>(
    snapshot: &'a MarketSnapshot,
    security: &'a BenchmarkSecurity,
) -> Result<BenchmarkInput<'a>, LevelError> {
    let unavailable = |reason| benchmark_unavailable(security, reason);
    let bond = held_benchmark(snapshot, security)
        .ok_or_else(|| unavailable("the runtime does not value its pinned version"))?;
    let clean_price_mid = snapshot
        .quotes_for(security.instrument_id.as_str())
        .iter()
        .find_map(|quote| {
            let field = quote.image.as_ref()?.fields.get(&QuoteField::MidPrice)?;
            Some(field.value.value)
        })
        .ok_or_else(|| unavailable("its quote has no mid price"))?;
    Ok(BenchmarkInput {
        security,
        bond,
        clean_price_mid,
    })
}

/// A bond with no rule is quoted by price.
fn price_without_rule(input: QuotedInput) -> Result<RulePricing, LevelError> {
    match input {
        QuotedInput::Price(price) => Ok(RulePricing {
            clean_price_mid: price,
            bid_ask: BidAsk::ConfigurationWidth,
            government_curve: None,
            g_spread_refused: None,
            benchmark: None,
        }),
        QuotedInput::Level(_) => Err(refused(
            "this bond has no pricing rule in force for its terms, so it is quoted by price; send market_price",
        )),
    }
}

/// The stage under a rule quoting `bond` by `method` with `width`, with the securities
/// of the set it names as read, when it names one. The width widens what the rule
/// quotes: a level for a level-quoted bond, a price for one quoted by price. With no
/// width, and for a price sent for a level-quoted bond, the pricing is mid only.
#[allow(clippy::too_many_arguments)]
fn price_with_rule(
    bond: &BondReferenceData,
    method: &PricingMethod,
    width: Option<&WidthSchedule>,
    benchmarks: Option<&[BenchmarkInput<'_>]>,
    input: QuotedInput,
    settlement: Date,
    discount: Option<&BuiltCurve>,
    yields: &BenchmarkYieldCache,
) -> Result<RulePricing, LevelError> {
    if (method.is_level() || benchmarks.is_some()) && !is_plain_bullet(bond) {
        return Err(refused(NOT_A_PLAIN_BULLET));
    }
    if width.is_some() && !is_quoted_two_way(bond) {
        return Err(refused(WIDTH_NEEDS_TWO_WAY));
    }
    // A request carries no trade size, so it takes the first bucket.
    let width = width
        .map(|schedule| schedule.width_for(None))
        .transpose()
        .map_err(width_refused)?;
    let benchmarks = match benchmarks {
        Some(set) => Some(price_benchmarks(set, settlement, yields)?),
        None => None,
    };
    let (clean_price_mid, benchmark, bid_ask) = match input {
        QuotedInput::Price(price) if method.is_level() => (price, None, BidAsk::MidOnly),
        QuotedInput::Price(price) => {
            let bid_ask = match width {
                Some(width) => {
                    let mid = Mark::Price {
                        value: price,
                        kind: PriceKind::Clean,
                    };
                    BidAsk::RuleWidth(two_way_at(width, &mid, |side| Ok(side.level()))?)
                }
                None => BidAsk::MidOnly,
            };
            (price, None, bid_ask)
        }
        QuotedInput::Level(level) => {
            let (clean, benchmark, two_way) = level_to_clean_price(
                bond,
                method,
                level,
                width,
                settlement,
                discount,
                benchmarks.as_deref(),
            )?;
            (
                clean,
                benchmark,
                two_way.map_or(BidAsk::MidOnly, BidAsk::RuleWidth),
            )
        }
    };
    let (government_curve, g_spread_refused) = match benchmarks {
        Some(all)
            if matures_within(
                bond.maturity_date,
                all.iter().map(|b| b.government_benchmark.maturity),
            ) =>
        {
            (
                Some(benchmark_curve(
                    all.into_iter().map(|b| b.government_benchmark).collect(),
                    settlement,
                    bond.currency,
                )?),
                None,
            )
        }
        Some(_) => (
            None,
            Some("the bond matures outside its benchmark set's maturities"),
        ),
        None => (None, None),
    };
    Ok(RulePricing {
        clean_price_mid,
        bid_ask,
        government_curve,
        g_spread_refused,
        benchmark,
    })
}

/// The two-way `width` gives around `mid`, each side's clean price from `clean_price`.
fn two_way_at(
    width: QuoteWidth,
    mid: &Mark,
    clean_price: impl Fn(&Mark) -> Result<Decimal, LevelError>,
) -> Result<TwoWay, LevelError> {
    let sides = width.two_way(mid).map_err(width_refused)?;
    Ok(TwoWay {
        width,
        bid_level: sides.bid.level(),
        ask_level: sides.ask.level(),
        clean_price_bid: clean_price(&sides.bid)?,
        clean_price_ask: clean_price(&sides.ask)?,
    })
}

/// The clean price a level gives under `method`, the benchmark a spread named, and, at
/// `width`, the two-way: each side's level priced as the mid is.
fn level_to_clean_price(
    bond: &BondReferenceData,
    method: &PricingMethod,
    level: Decimal,
    width: Option<QuoteWidth>,
    settlement: Date,
    discount: Option<&BuiltCurve>,
    benchmarks: Option<&[PricedBenchmark<'_>]>,
) -> Result<(Decimal, Option<BenchmarkYield>, Option<TwoWay>), LevelError> {
    // The level is checked before the bond is built.
    check_level(method, level)?;
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
                .ok_or_else(|| {
                    refused(
                        "a benchmark spread is to the benchmark set the bond's pricing rule names, and it names none",
                    )
                })?
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
                    tenor: quoted_to.security.tenor.clone(),
                    instrument_id: quoted_to.security.instrument_id.clone(),
                    ytm,
                }),
            )
        }
    };
    let clean_price = |mark: &Mark| {
        let priced = price_from_level(&fixed, settlement, mark, frequency, &curves)
            .map_err(analytics_error)?;
        Decimal::from_f64_retain(priced.clean_price_per_100)
            .ok_or_else(|| LevelError::Analytics("the level's price is not finite".into()))
    };
    let clean = clean_price(&mark)?;
    let two_way = width
        .map(|width| two_way_at(width, &mark, clean_price))
        .transpose()?;
    Ok((clean, benchmark, two_way))
}

/// Refuse a level outside its method's range: a yield sent in percent, or a spread in
/// decimal, is refused rather than priced.
pub(crate) fn check_level(method: &PricingMethod, level: Decimal) -> Result<(), LevelError> {
    let (range, unit) = match method {
        PricingMethod::Price {} => return Err(quoted_by_price()),
        PricingMethod::Yield {} => (YIELD_LEVEL_RANGE, "a decimal yield"),
        PricingMethod::ZSpread {} | PricingMethod::BenchmarkSpread { .. } => {
            (SPREAD_LEVEL_RANGE_BPS, "basis points of spread")
        }
    };
    if range.contains(&level) {
        Ok(())
    } else {
        Err(refused(format!(
            "a {} level is {unit} from {} to {}",
            method.name(),
            range.start(),
            range.end()
        )))
    }
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

fn width_refused(error: QuoteWidthError) -> LevelError {
    refused(error.reason())
}

fn refused(reason: impl Into<String>) -> LevelError {
    LevelError::Refused(reason.into())
}

fn analytics_error(error: impl std::fmt::Display) -> LevelError {
    LevelError::Analytics(error.to_string())
}

/// One benchmark security as the stage reads it: its entry in the set, its terms at the
/// version the set pins, and its clean price mid, borrowed from a rule in force or from a
/// snapshot.
struct BenchmarkInput<'a> {
    security: &'a BenchmarkSecurity,
    bond: &'a BondReferenceData,
    clean_price_mid: Decimal,
}

/// A benchmark security priced for one valuation: its entry in the set, its tenor in
/// months and the pure crate's record of it, which carries its street yield.
struct PricedBenchmark<'a> {
    security: &'a BenchmarkSecurity,
    months: u32,
    government_benchmark: GovernmentBenchmark,
}

/// Each security of a set with its street yield at `settlement`, from its clean price mid,
/// solved once for each price through `yields`. Each is a sovereign plain bullet in USD,
/// checked when the set was committed at the version it pins.
fn price_benchmarks<'a>(
    inputs: &[BenchmarkInput<'a>],
    settlement: Date,
    yields: &BenchmarkYieldCache,
) -> Result<Vec<PricedBenchmark<'a>>, LevelError> {
    inputs
        .iter()
        .map(|input| {
            let bond = input.bond;
            let street_yield = yields.street_yield(input, settlement)?;
            let security_id = bond
                .cusip
                .as_deref()
                .and_then(|c| SecurityId::cusip(c).ok())
                .or_else(|| bond.isin.as_deref().and_then(|i| SecurityId::isin(i).ok()))
                .ok_or_else(|| {
                    refused(format!(
                        "the {} benchmark has no valid CUSIP or ISIN",
                        input.security.tenor
                    ))
                })?;
            let months = tenor_months(&input.security.tenor).map_err(|e| refused(e.to_string()))?;
            Ok(PricedBenchmark {
                security: input.security,
                months,
                government_benchmark: GovernmentBenchmark::new(
                    Sovereign::UST,
                    Tenor::Custom(months),
                    security_id,
                    bond.maturity_date,
                    bond.coupon_rate.unwrap_or(Decimal::ZERO),
                    street_yield,
                )
                .with_price(input.clean_price_mid),
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

/// Whether `maturity` lies within a set's `maturities`, so the G-spread interpolates and
/// never extrapolates.
fn matures_within(maturity: Date, maturities: impl Iterator<Item = Date>) -> bool {
    let range = maturities.fold(None, |range: Option<(Date, Date)>, date| match range {
        None => Some((date, date)),
        Some((first, last)) => Some((first.min(date), last.max(date))),
    });
    range.is_some_and(|(first, last)| first <= maturity && maturity <= last)
}

/// Whether a valuation of the bond `reference` resolves in `snapshot` publishes a
/// G-spread: it is a plain bullet whose rule names a set the snapshot holds, with every
/// security at its pinned version, and it matures within their maturities. Otherwise
/// `g_spread_mid` is not asked of it, so its result stays complete.
pub fn is_g_spread_published(snapshot: &MarketSnapshot, reference: &ResolvedBondReference) -> bool {
    let (Some(bond), Some(rule)) = (&reference.record.bond, &reference.pricing_rule) else {
        return false;
    };
    if !is_plain_bullet(bond) || rule.benchmark_set_id.is_none() {
        return false;
    }
    let maturities = snapshot.benchmark_securities(rule).and_then(|securities| {
        securities
            .iter()
            .map(|security| held_benchmark(snapshot, security).map(|held| held.maturity_date))
            .try_fold(Vec::new(), |mut maturities, maturity| {
                maturities.push(maturity?);
                Some(maturities)
            })
    });
    maturities.is_some_and(|maturities| matures_within(bond.maturity_date, maturities.into_iter()))
}
