//! Book limit evaluation (P14a; D83, D84).
//!
//! [`evaluate_limits`] is a pure function of one book's positions, each position's
//! accepted runtime result read as an analyzer quote, the priced bond's classifications,
//! and the book's limit set. It reads no clock, store or runtime. Each aggregate is
//! computed once per evaluation, however many limits read it.
//!
//! Market values are admitted exactly as analytics by book admit them
//! (`portfolio_analytics::admit_quote`), so a limit on net market value, DV01 or modified
//! duration reads the number `POST /api/v1/portfolio/analytics` returns for the same book.
//! Every status is decided on exact state: a value exactly at a bound is within it, and a
//! ratio is compared as `numerator` against `bound × denominator`. Only the reported value
//! and utilisation are rounded.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use convex_core::contracts::{ExactDecimal, FactorTenor, InstrumentId};
use convex_core::{Currency, Date};
use convex_math::decimal::BoundedDecimal;
use rust_decimal::{Decimal, RoundingStrategy};

use crate::error::EngineError;
use crate::portfolio_analytics::admit_quote;
use crate::ports::limits::{
    validate_limits, InstrumentList, LimitDefinition, LimitEvaluation, LimitGroup, LimitMeasure,
    LimitMember, LimitStatus, UnknownReason, MAX_REASON_DETAIL_BYTES,
};
use crate::ports::output::BondQuoteOutput;
use crate::ports::reference_data::BondReferenceData;
use crate::ports::reference_master::{QuantityUnit, MAX_BOOK_POSITIONS};

/// Decimal places of a reported utilisation.
const UTILISATION_PLACES: u32 = 8;

/// The priced bond's classifications that group limits read.
#[derive(Debug, Clone, Copy)]
pub struct LimitTerms<'a> {
    /// `issuer_id`.
    pub issuer: &'a str,
    /// `country_of_risk`.
    pub country: &'a str,
    /// `sector`.
    pub sector: &'a str,
}

impl<'a> LimitTerms<'a> {
    /// The classifications of one priced bond version.
    #[must_use]
    pub fn of(bond: &'a BondReferenceData) -> Self {
        Self {
            issuer: &bond.issuer_id,
            country: &bond.country_of_risk,
            sector: &bond.sector,
        }
    }
}

/// One position as limits read it.
#[derive(Debug, Clone, Copy)]
pub struct LimitPosition<'a> {
    /// The resolved instrument.
    pub instrument_id: &'a InstrumentId,
    /// Signed quantity; negative is short.
    pub quantity: Decimal,
    /// Denomination of the quantity. Market measures and face limits take face.
    pub unit: QuantityUnit,
    /// The position's supplied rating.
    pub rating: Option<&'a str>,
    /// The accepted runtime result for the position, read as an analyzer quote; `None`
    /// when there is none.
    pub quote: Option<&'a BondQuoteOutput>,
    /// The priced bond's classifications; `None` when they are not known.
    pub terms: Option<LimitTerms<'a>>,
}

/// One book as limits read it.
#[derive(Debug, Clone, Copy)]
pub struct LimitBook<'a> {
    /// The book's reporting currency.
    pub currency: Option<Currency>,
    /// Positions, unique by instrument, at most `MAX_BOOK_POSITIONS`.
    pub positions: &'a [LimitPosition<'a>],
}

/// An exact value `n / d` with `d` positive; `whole` when `d` is one.
#[derive(Debug, Clone)]
struct Exact {
    n: BoundedDecimal,
    d: BoundedDecimal,
    whole: bool,
}

type Judged<T> = Result<T, UnknownReason>;

fn bound() -> UnknownReason {
    UnknownReason::ArithmeticBound {}
}

fn negate(value: &BoundedDecimal) -> Judged<BoundedDecimal> {
    value.multiply(Decimal::NEGATIVE_ONE).map_err(|_| bound())
}

impl Exact {
    fn whole(n: BoundedDecimal) -> Self {
        Self {
            n,
            d: BoundedDecimal::decimal(Decimal::ONE),
            whole: true,
        }
    }

    /// `n / d` for a nonzero `d`, with the sign moved to the numerator.
    fn ratio(n: BoundedDecimal, d: &BoundedDecimal) -> Judged<Self> {
        if d.is_negative() {
            return Ok(Self {
                n: negate(&n)?,
                d: negate(d)?,
                whole: false,
            });
        }
        Ok(Self {
            n,
            d: d.clone(),
            whole: false,
        })
    }

    /// Order of the value against `bound × fraction`, exactly.
    fn against(&self, bound: Decimal, fraction: Option<Decimal>) -> Judged<Ordering> {
        let mut scaled = self.d.multiply(bound).map_err(|_| self::bound())?;
        if let Some(fraction) = fraction {
            scaled = scaled.multiply(fraction).map_err(|_| self::bound())?;
        }
        Ok(self.n.cmp(&scaled))
    }

    /// The value, rounded half to even at Decimal precision.
    fn report(&self) -> Judged<ExactDecimal> {
        let value = if self.whole {
            self.n.finish()
        } else {
            self.n.ratio(&self.d)
        };
        value
            .map(|v| ExactDecimal::new(v.normalize()))
            .map_err(|_| bound())
    }

    /// The value over `limit`, which faces away from zero.
    fn over(&self, limit: Decimal) -> Judged<Decimal> {
        let (n, limit) = if limit.is_sign_negative() {
            (negate(&self.n)?, -limit)
        } else {
            (self.n.clone(), limit)
        };
        let d = self.d.multiply(limit).map_err(|_| bound())?;
        n.ratio(&d)
            .map(|u| {
                u.round_dp_with_strategy(UTILISATION_PLACES, RoundingStrategy::MidpointNearestEven)
            })
            .map_err(|_| bound())
    }
}

/// A limit's status for one exact value, and the utilisation it reports.
fn judge(limit: &LimitDefinition, value: &Exact) -> Judged<(LimitStatus, Option<Decimal>)> {
    let lower = limit.lower.map(ExactDecimal::decimal);
    let upper = limit.upper.map(ExactDecimal::decimal);
    let fraction = limit.warning_fraction.map(ExactDecimal::decimal);
    let past = |bound: Option<Decimal>, fraction: Option<Decimal>, side: Ordering| {
        bound
            .map(|b| value.against(b, fraction).map(|o| o == side))
            .transpose()
            .map(|past| past.unwrap_or(false))
    };
    // Validation keeps a warning band to bounds that face away from zero.
    let status = if past(upper, None, Ordering::Greater)? || past(lower, None, Ordering::Less)? {
        LimitStatus::Breach
    } else if fraction.is_some()
        && (past(upper, fraction, Ordering::Greater)? || past(lower, fraction, Ordering::Less)?)
    {
        LimitStatus::Warning
    } else {
        LimitStatus::Within
    };
    let mut utilisation: Option<Decimal> = None;
    for facing in [
        upper.filter(|u| *u > Decimal::ZERO),
        lower.filter(|l| *l < Decimal::ZERO),
    ]
    .into_iter()
    .flatten()
    {
        let u = value.over(facing)?;
        utilisation = Some(utilisation.map_or(u, |v| v.max(u)));
    }
    Ok((status, utilisation))
}

/// One member's evaluation, before it is chosen as a limit's.
struct Candidate {
    status: LimitStatus,
    utilisation: Option<Decimal>,
    value: Exact,
    member: Option<String>,
}

fn candidate(limit: &LimitDefinition, value: Exact, member: Option<String>) -> Judged<Candidate> {
    let (status, utilisation) = judge(limit, &value)?;
    Ok(Candidate {
        status,
        utilisation,
        value,
        member,
    })
}

/// The worse of two candidates: the more severe status, then the larger utilisation;
/// the earlier on a tie.
fn worse(a: Candidate, b: Candidate) -> Candidate {
    match (b.status, b.utilisation).cmp(&(a.status, a.utilisation)) {
        Ordering::Greater => b,
        _ => a,
    }
}

fn known(limit: &LimitDefinition, found: Option<Candidate>) -> Judged<LimitEvaluation> {
    let Some(found) = found else {
        // A limit on each member of a book that holds none.
        return Ok(LimitEvaluation {
            limit_id: limit.limit_id.clone(),
            status: LimitStatus::Within,
            value: None,
            member: None,
            utilisation: None,
            reason: None,
        });
    };
    Ok(LimitEvaluation {
        limit_id: limit.limit_id.clone(),
        status: found.status,
        value: Some(found.value.report()?),
        member: found.member,
        utilisation: found.utilisation.map(|u| ExactDecimal::new(u.normalize())),
        reason: None,
    })
}

fn unknown(limit: &LimitDefinition, reason: UnknownReason) -> LimitEvaluation {
    LimitEvaluation {
        limit_id: limit.limit_id.clone(),
        status: LimitStatus::Unknown,
        value: None,
        member: None,
        utilisation: None,
        reason: Some(reason),
    }
}

/// An admitted position.
struct Row<'a> {
    position: &'a LimitPosition<'a>,
    quote: &'a BondQuoteOutput,
    /// Dirty market value, in book currency.
    value: BoundedDecimal,
}

/// The book's admitted market values.
struct Market<'a> {
    rows: Vec<Row<'a>>,
    net: BoundedDecimal,
    gross: BoundedDecimal,
}

fn listed<'a>(
    positions: &'a [LimitPosition<'a>],
    keep: impl Fn(&LimitPosition<'a>) -> bool,
) -> Option<InstrumentList> {
    InstrumentList::collect(
        positions
            .iter()
            .filter(|p| keep(p))
            .map(|p| p.instrument_id),
    )
}

fn detail(error: &EngineError) -> String {
    let mut text = error.to_string();
    if text.len() > MAX_REASON_DETAIL_BYTES {
        let mut cut = MAX_REASON_DETAIL_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    text
}

/// Admit the book's market values, after the book-level reasons in D84's order.
fn admit_market<'a>(
    positions: &'a [LimitPosition<'a>],
    currency: Option<Currency>,
) -> Judged<Market<'a>> {
    let Some(currency) = currency else {
        return Err(UnknownReason::BookCurrencyRequired {});
    };
    if let Some(instruments) = listed(positions, |p| p.unit != QuantityUnit::Face) {
        return Err(UnknownReason::UnsupportedQuantityUnit { instruments });
    }
    if let Some(instruments) = listed(positions, |p| p.quote.is_none()) {
        return Err(UnknownReason::MissingResult { instruments });
    }
    if let Some(instruments) = listed(positions, |p| {
        p.quote.is_some_and(|q| q.currency != currency)
    }) {
        return Err(UnknownReason::ForeignCurrency { instruments });
    }
    let mut rows = Vec::with_capacity(positions.len());
    let mut net = BoundedDecimal::zero();
    let mut gross = BoundedDecimal::zero();
    let mut date: Option<Date> = None;
    let mut timestamp: Option<i64> = None;
    for (position, quote) in positions
        .iter()
        .filter_map(|p| p.quote.map(|quote| (p, quote)))
    {
        let id = position.instrument_id.as_str();
        let value = admit_quote(
            id,
            position.quantity,
            quote,
            currency,
            &mut date,
            &mut timestamp,
        )
        .map_err(|error| UnknownReason::QuoteRefused {
            instrument: position.instrument_id.clone(),
            detail: detail(&error),
        })?;
        net.add(&value).map_err(|_| bound())?;
        let size = if value.is_negative() {
            negate(&value)?
        } else {
            value.clone()
        };
        gross.add(&size).map_err(|_| bound())?;
        rows.push(Row {
            position,
            quote,
            value,
        });
    }
    Ok(Market { rows, net, gross })
}

fn not_computed(
    measure: String,
    rows: &[Row<'_>],
    missing: impl Fn(&Row<'_>) -> bool,
) -> Judged<()> {
    match InstrumentList::collect(
        rows.iter()
            .filter(|r| missing(r))
            .map(|r| r.position.instrument_id),
    ) {
        Some(instruments) => Err(UnknownReason::MeasureNotComputed {
            measure,
            instruments,
        }),
        None => Ok(()),
    }
}

/// A per-100-face sensitivity summed over the book: `Σ measure × face / 100`.
fn per_face(
    market: &Market<'_>,
    measure: &str,
    get: impl Fn(&BondQuoteOutput) -> Option<Decimal>,
) -> Judged<Exact> {
    not_computed(measure.to_owned(), &market.rows, |r| get(r.quote).is_none())?;
    let mut sum = BoundedDecimal::zero();
    for row in &market.rows {
        let Some(per_100) = get(row.quote) else {
            continue;
        };
        let term = BoundedDecimal::decimal(per_100)
            .multiply(row.position.quantity)
            .and_then(|v| v.divide_power_of_ten(2))
            .map_err(|_| bound())?;
        sum.add(&term).map_err(|_| bound())?;
    }
    Ok(Exact::whole(sum))
}

fn key_rate_duration(quote: &BondQuoteOutput, tenor: &FactorTenor) -> Option<Decimal> {
    quote
        .key_rate_durations
        .as_ref()?
        .iter()
        .find(|(label, _)| label == tenor.as_str())
        .map(|(_, value)| *value)
}

/// Key-rate DV01 at one tenor: `Σ value × key-rate duration / 10,000`.
fn key_rate_dv01(market: &Market<'_>, tenor: &FactorTenor) -> Judged<Exact> {
    let measure = format!("krd_{}_mid", tenor.as_str().to_ascii_lowercase());
    not_computed(measure, &market.rows, |r| {
        key_rate_duration(r.quote, tenor).is_none()
    })?;
    let mut sum = BoundedDecimal::zero();
    for row in &market.rows {
        let Some(duration) = key_rate_duration(row.quote, tenor) else {
            continue;
        };
        let term = row
            .value
            .multiply(duration)
            .and_then(|v| v.divide_power_of_ten(4))
            .map_err(|_| bound())?;
        sum.add(&term).map_err(|_| bound())?;
    }
    Ok(Exact::whole(sum))
}

/// Modified duration weighted by net market value, as the analyzer weights it.
fn modified_duration(market: &Market<'_>) -> Judged<Exact> {
    not_computed("modified_duration_mid".to_owned(), &market.rows, |r| {
        r.quote.modified_duration.is_none()
    })?;
    if market.net.is_zero() {
        return Err(UnknownReason::ZeroNetValue {});
    }
    let mut sum = BoundedDecimal::zero();
    for row in &market.rows {
        let Some(duration) = row.quote.modified_duration else {
            continue;
        };
        sum.add(&row.value.multiply(duration).map_err(|_| bound())?)
            .map_err(|_| bound())?;
    }
    Exact::ratio(sum, &market.net)
}

fn classification<'a>(position: &LimitPosition<'a>, by: LimitGroup) -> Option<&'a str> {
    let label = match by {
        LimitGroup::Issuer => position.terms.map(|t| t.issuer),
        LimitGroup::Country => position.terms.map(|t| t.country),
        LimitGroup::Sector => position.terms.map(|t| t.sector),
        LimitGroup::Rating => position.rating,
    };
    label.filter(|label| !label.trim().is_empty())
}

/// Net market value by group. Every position must carry the classification: one without
/// it could belong to any group.
fn groups<'a>(market: &Market<'a>, by: LimitGroup) -> Judged<BTreeMap<&'a str, BoundedDecimal>> {
    if let Some(instruments) = InstrumentList::collect(
        market
            .rows
            .iter()
            .filter(|r| classification(r.position, by).is_none())
            .map(|r| r.position.instrument_id),
    ) {
        return Err(UnknownReason::MissingClassification { by, instruments });
    }
    let mut sums = BTreeMap::<&'a str, BoundedDecimal>::new();
    for row in &market.rows {
        let Some(label) = classification(row.position, by) else {
            continue;
        };
        sums.entry(label)
            .or_insert_with(BoundedDecimal::zero)
            .add(&row.value)
            .map_err(|_| bound())?;
    }
    Ok(sums)
}

/// The aggregates of one evaluation, each computed at most once.
#[derive(Default)]
struct Aggregates<'a> {
    dv01: Option<Judged<Exact>>,
    cs01: Option<Judged<Exact>>,
    duration: Option<Judged<Exact>>,
    key_rates: Vec<(FactorTenor, Judged<Exact>)>,
    groups: BTreeMap<LimitGroup, Judged<BTreeMap<&'a str, BoundedDecimal>>>,
}

fn cached<T: Clone>(slot: &mut Option<T>, compute: impl FnOnce() -> T) -> T {
    slot.get_or_insert_with(compute).clone()
}

/// The extreme members of a limit on each: the largest and the smallest, judged against
/// the limit, and the worse of the two kept; the earlier member on a tie. `None` when
/// there are none. `key` orders the members as their values do (every member shares one
/// positive denominator), and only the two extremes are lifted to exact values.
fn extremes<'m, K: Ord + Copy>(
    limit: &LimitDefinition,
    members: impl Iterator<Item = (K, &'m str)>,
    lift: impl Fn(K) -> Judged<Exact>,
) -> Judged<Option<Candidate>> {
    let mut largest: Option<(K, &'m str)> = None;
    let mut smallest: Option<(K, &'m str)> = None;
    for (key, member) in members {
        if largest.is_none_or(|(k, _)| key > k) {
            largest = Some((key, member));
        }
        if smallest.is_none_or(|(k, _)| key < k) {
            smallest = Some((key, member));
        }
    }
    let mut found: Option<Candidate> = None;
    for (key, member) in [largest, smallest].into_iter().flatten() {
        let next = candidate(limit, lift(key)?, Some(member.to_owned()))?;
        found = Some(match found {
            Some(current) => worse(current, next),
            None => next,
        });
    }
    Ok(found)
}

fn face_value(position: &LimitPosition<'_>) -> Exact {
    Exact::whole(BoundedDecimal::decimal(position.quantity))
}

fn face<'a>(
    limit: &LimitDefinition,
    book: &LimitBook<'a>,
    by_instrument: &HashMap<&'a str, &'a LimitPosition<'a>>,
    instrument: &LimitMember<InstrumentId>,
) -> Judged<LimitEvaluation> {
    match instrument {
        LimitMember::One(id) => match by_instrument.get(id.as_str()) {
            Some(p) if p.unit != QuantityUnit::Face => {
                Err(UnknownReason::UnsupportedQuantityUnit {
                    instruments: InstrumentList {
                        count: 1,
                        first: vec![p.instrument_id.clone()],
                    },
                })
            }
            Some(p) => known(limit, Some(candidate(limit, face_value(p), None)?)),
            None => known(
                limit,
                Some(candidate(
                    limit,
                    Exact::whole(BoundedDecimal::zero()),
                    None,
                )?),
            ),
        },
        LimitMember::Each => {
            // Quantities order exactly as Decimals.
            let found = extremes(
                limit,
                book.positions
                    .iter()
                    .filter(|p| p.unit == QuantityUnit::Face)
                    .map(|p| (p.quantity, p.instrument_id.as_str())),
                |quantity| Ok(Exact::whole(BoundedDecimal::decimal(quantity))),
            )?;
            let breach = found
                .as_ref()
                .is_some_and(|c| c.status == LimitStatus::Breach);
            match listed(book.positions, |p| p.unit != QuantityUnit::Face) {
                // A known breach outranks a member that cannot be seen.
                Some(instruments) if !breach => {
                    Err(UnknownReason::UnsupportedQuantityUnit { instruments })
                }
                _ => known(limit, found),
            }
        }
    }
}

fn group(
    limit: &LimitDefinition,
    market: &Market<'_>,
    sums: &BTreeMap<&str, BoundedDecimal>,
    member: &LimitMember<String>,
    share: bool,
) -> Judged<LimitEvaluation> {
    if share && market.gross.is_zero() {
        return Err(UnknownReason::ZeroGrossValue {});
    }
    let value = |sum: &BoundedDecimal| {
        if share {
            Exact::ratio(sum.clone(), &market.gross)
        } else {
            Ok(Exact::whole(sum.clone()))
        }
    };
    match member {
        LimitMember::One(key) => {
            let zero = BoundedDecimal::zero();
            let sum = sums.get(key.as_str()).unwrap_or(&zero);
            known(limit, Some(candidate(limit, value(sum)?, None)?))
        }
        LimitMember::Each => known(
            limit,
            extremes(limit, sums.iter().map(|(key, sum)| (sum, *key)), value)?,
        ),
    }
}

fn whole_book(limit: &LimitDefinition, value: Judged<Exact>) -> Judged<LimitEvaluation> {
    known(limit, Some(candidate(limit, value?, None)?))
}

/// What one evaluation shares between its limits.
struct Evaluation<'a, 'b> {
    book: &'b LimitBook<'a>,
    by_instrument: HashMap<&'a str, &'a LimitPosition<'a>>,
    /// Admitted on the first limit that reads market data.
    market: Option<Judged<Market<'a>>>,
    aggregates: Aggregates<'a>,
}

impl<'a> Evaluation<'a, '_> {
    fn limit(&mut self, limit: &LimitDefinition) -> Judged<LimitEvaluation> {
        if let LimitMeasure::Face { instrument } = &limit.measure {
            return face(limit, self.book, &self.by_instrument, instrument);
        }
        let (positions, currency) = (self.book.positions, self.book.currency);
        let market = match self
            .market
            .get_or_insert_with(|| admit_market(positions, currency))
        {
            Ok(market) => &*market,
            Err(reason) => return Err(reason.clone()),
        };
        let aggregates = &mut self.aggregates;
        match &limit.measure {
            LimitMeasure::Face { instrument } => {
                face(limit, self.book, &self.by_instrument, instrument)
            }
            LimitMeasure::MarketValue {} => whole_book(limit, Ok(Exact::whole(market.net.clone()))),
            LimitMeasure::GrossMarketValue {} => {
                whole_book(limit, Ok(Exact::whole(market.gross.clone())))
            }
            LimitMeasure::Dv01 {} => whole_book(
                limit,
                cached(&mut aggregates.dv01, || {
                    per_face(market, "dv01_mid", |q| q.dv01)
                }),
            ),
            LimitMeasure::Cs01 {} => whole_book(
                limit,
                cached(&mut aggregates.cs01, || {
                    per_face(market, "cs01_mid", |q| q.cs01)
                }),
            ),
            LimitMeasure::KeyRateDv01 { tenor } => {
                let value = match aggregates.key_rates.iter().find(|(t, _)| t == tenor) {
                    Some((_, value)) => value.clone(),
                    None => {
                        let value = key_rate_dv01(market, tenor);
                        aggregates.key_rates.push((tenor.clone(), value.clone()));
                        value
                    }
                };
                whole_book(limit, value)
            }
            LimitMeasure::ModifiedDuration {} => whole_book(
                limit,
                cached(&mut aggregates.duration, || modified_duration(market)),
            ),
            LimitMeasure::GroupMarketValue { by, group: member } => {
                match aggregates
                    .groups
                    .entry(*by)
                    .or_insert_with(|| groups(market, *by))
                {
                    Ok(sums) => group(limit, market, sums, member, false),
                    Err(reason) => Err(reason.clone()),
                }
            }
            LimitMeasure::GroupShare { by, group: member } => {
                match aggregates
                    .groups
                    .entry(*by)
                    .or_insert_with(|| groups(market, *by))
                {
                    Ok(sums) => group(limit, market, sums, member, true),
                    Err(reason) => Err(reason.clone()),
                }
            }
        }
    }
}

/// Evaluate each limit of a book's set against the book, in set order.
///
/// Refuses (an error, not an `unknown` status) only input that breaks the contract: an
/// invalid limit set, more than `MAX_BOOK_POSITIONS` positions, or an instrument held
/// twice. Every missing input is an `unknown` status with its reason; a face limit is
/// still evaluated when results are missing.
pub fn evaluate_limits<'a>(
    book: &LimitBook<'a>,
    limits: &[LimitDefinition],
) -> Result<Vec<LimitEvaluation>, EngineError> {
    validate_limits(limits)
        .map_err(|error| EngineError::ConfigError(format!("limit set: {error}")))?;
    if book.positions.len() > MAX_BOOK_POSITIONS {
        return Err(EngineError::PricingError(format!(
            "limits: a book holds at most {MAX_BOOK_POSITIONS} positions"
        )));
    }
    let mut by_instrument = HashMap::with_capacity(book.positions.len());
    for position in book.positions {
        if by_instrument
            .insert(position.instrument_id.as_str(), position)
            .is_some()
        {
            return Err(EngineError::PricingError(format!(
                "limits: instrument {} is held twice",
                position.instrument_id
            )));
        }
    }
    let mut evaluation = Evaluation {
        book,
        by_instrument,
        market: None,
        aggregates: Aggregates::default(),
    };
    Ok(limits
        .iter()
        .map(|limit| {
            evaluation
                .limit(limit)
                .unwrap_or_else(|reason| unknown(limit, reason))
        })
        .collect())
}
