//! Pricing rules and benchmark sets: how a stored bond is quoted, and the
//! government securities a spread is quoted to.
//!
//! A bond's [`PricingRuleDefinition`] (`MasterKey::PricingRule`) names its method: a price,
//! a yield, a spread to one benchmark security, or a Z-spread. A callable quoted by price
//! or by its yield to worst is valued on its workouts. The level itself is a quote,
//! never stored. A rule may name a [`BenchmarkSetDefinition`]
//! (`MasterKey::BenchmarkSet`) by id, so an on-the-run roll is one commit to the set and
//! the version a valuation read is recorded with its result. A bond with no rule is
//! quoted by price. A rule may carry a two-way width in the unit it quotes, which a
//! request reads with the rule ([`RequestPricingRule`]) and a snapshot's pin leaves out.
//!
//! These are types, bounds and validation only. Turning a level into a price is the
//! engine's level stage, or its workout model for a callable, through the pure crates.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeSet;

use convex_core::contracts::{InstrumentId, ReferenceDataId, UtcTimestamp, Version};
use convex_core::futures::InstrumentPin;
use convex_core::types::{QuoteBasis, QuoteWidthError, WidthSchedule};
use convex_core::Currency;
use serde::{Deserialize, Serialize};

use crate::futures_reference::exact_bond;
use crate::quote_event::QuoteField;
use crate::reference_data::{BondReferenceData, BondType, IssuerType};
use crate::reference_master::{
    bond_at_commit, ensure_master_exists, CallStyle, EffectiveInterval, MasterDefinition,
    MasterKey, MasterRecord, ReferenceProvenance,
};
use crate::reference_repository::{ReferenceCommit, ReferenceError, ReferenceRecord};

/// Securities in one benchmark set.
pub const MAX_BENCHMARK_SECURITIES: usize = 32;
/// The longest benchmark tenor, in months: 50 years.
pub const MAX_BENCHMARK_TENOR_MONTHS: u32 = 600;
/// Benchmark sets one snapshot holds.
pub const MAX_SNAPSHOT_BENCHMARK_SETS: usize = 64;
/// Call dates a callable valued on its workouts may have.
pub const MAX_WORKOUT_CALLS: usize = 128;

/// How a stored bond is quoted. For a fixed bullet, each method other than `price` is a
/// level that the level stage turns into a price before the bond's model values it. A
/// callable quoted by `price` or `yield` is valued on its workouts, from its price or its
/// yield to worst; with no rule, or any other, it is valued on HW1F trees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PricingMethod {
    /// A clean price per 100 face, as a bond with no rule is quoted.
    Price {},
    /// A street yield to maturity, at the bond's coupon frequency; for a callable, its
    /// yield to worst.
    Yield {},
    /// A spread, in basis points, to the yield of one security of the rule's benchmark
    /// set, named by its tenor, both yields compounded semi-annually: for a Treasury, its
    /// street yield. This is how USD investment grade is quoted.
    BenchmarkSpread {
        /// The tenor of the set's security, such as `10Y`.
        tenor: String,
    },
    /// A constant spread, in basis points, over the configuration's discount curve.
    ZSpread {},
}

impl PricingMethod {
    /// Whether the method quotes a level (a yield or a spread) rather than a price.
    pub fn is_level(&self) -> bool {
        !matches!(self, Self::Price {})
    }

    /// The method's name on the wire, for refusals and provenance.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Price {} => "price",
            Self::Yield {} => "yield",
            Self::BenchmarkSpread { .. } => "benchmark_spread",
            Self::ZSpread {} => "z_spread",
        }
    }

    /// The mid quote field a bond quoted this way is read from.
    pub fn quote_field(&self) -> QuoteField {
        match self {
            Self::Price {} => QuoteField::MidPrice,
            Self::Yield {} => QuoteField::MidYield,
            Self::BenchmarkSpread { .. } | Self::ZSpread {} => QuoteField::MidSpread,
        }
    }

    /// The quote field of the level a bond quoted this way is read from; none for a price.
    pub fn level_field(&self) -> Option<QuoteField> {
        self.is_level().then(|| self.quote_field())
    }

    /// Whether a rule quoting this way, against a benchmark set when `names_benchmark_set`,
    /// applies to `bond`: a fixed bullet by every method; any other bond by price with no
    /// set; and a callable valued on its workouts by its yield to worst too.
    pub fn applies_to(&self, names_benchmark_set: bool, bond: &BondReferenceData) -> bool {
        is_plain_bullet(bond)
            || (!names_benchmark_set
                && match self {
                    Self::Price {} => true,
                    Self::Yield {} => is_workout_callable(bond),
                    Self::BenchmarkSpread { .. } | Self::ZSpread {} => false,
                })
    }

    /// What a bond quoted this way is quoted in: a price, a yield or a spread.
    pub fn quote_basis(&self) -> QuoteBasis {
        match self {
            Self::Price {} => QuoteBasis::Price,
            Self::Yield {} => QuoteBasis::Yield,
            Self::BenchmarkSpread { .. } | Self::ZSpread {} => QuoteBasis::Spread,
        }
    }

    /// Whether pricing a level this way reads the configuration's discount curve.
    pub fn reads_discount_curve(&self) -> bool {
        matches!(self, Self::ZSpread {})
    }

    /// Validate the method with the set it is quoted against: a benchmark spread names
    /// its set and a well-formed tenor.
    fn validate(&self, benchmark_set_id: Option<&ReferenceDataId>) -> Result<(), ReferenceError> {
        match self {
            Self::BenchmarkSpread { tenor } => {
                if benchmark_set_id.is_none() {
                    return Err(invalid("a benchmark spread names its benchmark set"));
                }
                tenor_months(tenor).map(|_| ())
            }
            Self::Price {} | Self::Yield {} | Self::ZSpread {} => Ok(()),
        }
    }
}

/// How one stored bond is quoted. Versioned; changed rarely. The level a
/// trader moves is a quote, not part of the rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PricingRuleDefinition {
    /// The bond quoted; one rule an instrument.
    pub instrument_id: InstrumentId,
    /// How its level is quoted.
    pub method: PricingMethod,
    /// The benchmark set its spread is quoted to, and its G-spread is computed over.
    /// Named by id: the version in force at a valuation's cutoff is read and recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_set_id: Option<ReferenceDataId>,
    /// The two-way width by trade size, in the unit the method quotes: price points or
    /// 32nds for a price, basis points for a yield or a spread. Applied at request time
    /// to a bond quoted in that unit; with none, a bond quoted by price takes the
    /// configuration's percent-of-price width and a level-quoted bond is quoted mid only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<WidthSchedule>,
    /// Validity; a bounded end retires the rule, and the bond is quoted by price again
    /// ([`NewestStarted`](crate::reference_master::VersionSelection::NewestStarted)).
    pub effective: EffectiveInterval,
    /// Source evidence.
    pub provenance: ReferenceProvenance,
}

impl PricingRuleDefinition {
    /// Whether the rule applies to `bond` ([`PricingMethod::applies_to`]).
    pub fn applies_to(&self, bond: &BondReferenceData) -> bool {
        self.method
            .applies_to(self.benchmark_set_id.is_some(), bond)
    }

    /// Validate the rule on its own: a benchmark spread names its set and a well-formed
    /// tenor, and a width is a valid schedule in a unit the method quotes. Whether the
    /// bond takes the method and the width is checked against the stored bond.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        self.method.validate(self.benchmark_set_id.as_ref())?;
        if let Some(width) = &self.width {
            width.validate().map_err(|error| invalid(error.reason()))?;
            if !width.unit.quotes(self.method.quote_basis()) {
                return Err(invalid(QuoteWidthError::UnitMismatch.reason()));
            }
        }
        Ok(())
    }
}

/// One government security of a benchmark set: the stored bond version it reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSecurity {
    /// Its tenor, unique in the set: months or years such as `6M` or `10Y`.
    pub tenor: String,
    /// The stored bond.
    pub instrument_id: InstrumentId,
    /// The exact stored version read.
    pub version: Version,
}

/// A versioned set of government securities that spreads are quoted to. Rolling a
/// benchmark to a new issue is one commit of a new version. Spreads are quoted against US
/// Treasuries only, so a set is in USD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSetDefinition {
    /// The set's identity, such as `UST-OTR`.
    pub benchmark_set_id: ReferenceDataId,
    /// The currency of every security in it.
    pub currency: Currency,
    /// 1 to [`MAX_BENCHMARK_SECURITIES`] securities, with unique tenors and instruments.
    pub securities: Vec<BenchmarkSecurity>,
    /// Validity; a bounded end retires the set
    /// ([`NewestStarted`](crate::reference_master::VersionSelection::NewestStarted)).
    pub effective: EffectiveInterval,
    /// Source evidence.
    pub provenance: ReferenceProvenance,
}

impl BenchmarkSetDefinition {
    /// Validate the set on its own: its size, and unique tenors and instruments, so `12M`
    /// and `1Y` are one tenor. That each security is a stored government bullet is checked
    /// against the stored bonds.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        if self.currency != Currency::USD {
            return Err(invalid(
                "a benchmark set is in USD: spreads are quoted against Treasuries only",
            ));
        }
        validate_securities(&self.securities)
    }
}

/// A set's securities: 1 to [`MAX_BENCHMARK_SECURITIES`], with unique tenors (`12M` and
/// `1Y` are one) and instruments.
fn validate_securities(securities: &[BenchmarkSecurity]) -> Result<(), ReferenceError> {
    if securities.is_empty() {
        return Err(invalid("a benchmark set holds at least one security"));
    }
    if securities.len() > MAX_BENCHMARK_SECURITIES {
        return Err(invalid("a benchmark set holds at most 32 securities"));
    }
    let mut tenors = BTreeSet::new();
    let mut instruments = BTreeSet::new();
    for security in securities {
        if !tenors.insert(tenor_months(&security.tenor)?) {
            return Err(invalid("duplicate benchmark tenor"));
        }
        if !instruments.insert(security.instrument_id.as_str()) {
            return Err(invalid("duplicate benchmark security"));
        }
    }
    Ok(())
}

/// A bond's pricing rule in force at the cutoff a valuation resolved it at: what pricing
/// reads of it, pinned by its version. A snapshot carries it beside the bond's reference,
/// and each set it names once, in its benchmark sets, so a Live activation pins both with
/// the references. A pinned rule is not checked against its effective interval at later
/// cutoffs: it stays in force until the next activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPricingRule {
    /// The rule's version in force.
    pub version: Version,
    /// When that version was recorded.
    pub recorded_at: UtcTimestamp,
    /// How the bond is quoted.
    pub method: PricingMethod,
    /// The set its spread is quoted to and its G-spread computed over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_set_id: Option<ReferenceDataId>,
}

impl TryFrom<&MasterRecord> for ResolvedPricingRule {
    type Error = ReferenceError;
    /// What pricing reads of a stored rule record.
    fn try_from(record: &MasterRecord) -> Result<Self, Self::Error> {
        let rule = rule_definition(record)?;
        Ok(Self {
            version: record.version,
            recorded_at: record.recorded_at,
            method: rule.method.clone(),
            benchmark_set_id: rule.benchmark_set_id.clone(),
        })
    }
}

/// A stored rule version as a request reads it: the pin a snapshot holds, and the width
/// the pin leaves out, since Live publishes mid measures only and the version names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPricingRule {
    /// The rule's pin: its version, method and set.
    pub pin: ResolvedPricingRule,
    /// Its two-way width at that version.
    pub width: Option<WidthSchedule>,
}

impl TryFrom<&MasterRecord> for RequestPricingRule {
    type Error = ReferenceError;
    /// What a request reads of a stored rule record.
    fn try_from(record: &MasterRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            pin: ResolvedPricingRule::try_from(record)?,
            width: rule_definition(record)?.width.clone(),
        })
    }
}

/// The rule a stored record holds.
fn rule_definition(record: &MasterRecord) -> Result<&PricingRuleDefinition, ReferenceError> {
    match &record.definition {
        MasterDefinition::PricingRule(rule) => Ok(rule),
        _ => Err(invalid("a resolved pricing rule is a pricing rule")),
    }
}

impl ResolvedPricingRule {
    /// Whether the rule applies to `bond` ([`PricingMethod::applies_to`]).
    pub fn applies_to(&self, bond: &BondReferenceData) -> bool {
        self.method
            .applies_to(self.benchmark_set_id.is_some(), bond)
    }

    /// Check the rule on its own and against `known_at`, the cutoff it was resolved by.
    pub fn validate(&self, known_at: UtcTimestamp) -> Result<(), ReferenceError> {
        self.method.validate(self.benchmark_set_id.as_ref())?;
        if self.recorded_at > known_at {
            return Err(invalid("a resolved pricing rule is recorded by the cutoff"));
        }
        Ok(())
    }
}

/// A benchmark set in force at the cutoff a valuation resolved it at: what pricing reads of
/// it, pinned by its version. A snapshot holds each set its rules name once; a Live
/// activation pins it with the references. Its currency was checked when it was committed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedBenchmarkSet {
    /// The set's identity.
    pub benchmark_set_id: ReferenceDataId,
    /// Its version in force.
    pub version: Version,
    /// When that version was recorded.
    pub recorded_at: UtcTimestamp,
    /// Its securities, in the set's order.
    pub securities: Vec<BenchmarkSecurity>,
}

impl TryFrom<&MasterRecord> for ResolvedBenchmarkSet {
    type Error = ReferenceError;
    /// What pricing reads of a stored set record.
    fn try_from(record: &MasterRecord) -> Result<Self, Self::Error> {
        let MasterDefinition::BenchmarkSet(set) = &record.definition else {
            return Err(invalid("a resolved benchmark set is a benchmark set"));
        };
        Ok(Self {
            benchmark_set_id: set.benchmark_set_id.clone(),
            version: record.version,
            recorded_at: record.recorded_at,
            securities: set.securities.clone(),
        })
    }
}

impl ResolvedBenchmarkSet {
    /// Check the set on its own and against `known_at`, the cutoff it was resolved by.
    pub fn validate(&self, known_at: UtcTimestamp) -> Result<(), ReferenceError> {
        validate_securities(&self.securities)?;
        if self.recorded_at > known_at {
            return Err(invalid(
                "a resolved benchmark set is recorded by the cutoff",
            ));
        }
        Ok(())
    }
}

/// Check a commit's rules and sets against the bonds and sets they name, each as of the
/// commit: a rule's bond is stored, and the rule applies to it
/// ([`PricingMethod::applies_to`]); a named set exists; each
/// security of a set is a stored government fixed bullet in the set's currency. A rule
/// version that has ended by its commit is checked only for its bond's identity, not its
/// bond's current terms, so a rule can be ended after its bond is deleted or changed:
/// pricing checks the bond it prices when it prices it.
pub(crate) fn validate_level_pricing_links(
    commit: &ReferenceCommit,
    master: &mut impl FnMut(&MasterKey, Version) -> Result<MasterRecord, ReferenceError>,
    bond_current: &mut impl FnMut(
        &convex_core::ids::InstrumentId,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>,
    bond_version: &mut impl FnMut(
        &convex_core::ids::InstrumentId,
        Version,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>,
) -> Result<(), ReferenceError> {
    for record in &commit.master_records {
        match &record.definition {
            MasterDefinition::PricingRule(rule) => {
                if rule.effective.has_ended_by(commit.recorded_at) {
                    // The bond's first revision proves its identity, whatever it is now.
                    let first = InstrumentPin {
                        id: rule.instrument_id.clone(),
                        version: Version::new(1).map_err(|_| invalid("invalid version"))?,
                    };
                    match exact_bond(commit, &first, bond_version) {
                        Err(ReferenceError::InvalidInput(_)) => {
                            return Err(invalid("a pricing rule names a stored bond"));
                        }
                        result => result.map(|_| ())?,
                    }
                } else {
                    let id: convex_core::ids::InstrumentId = rule.instrument_id.clone().into();
                    let bond = bond_at_commit(commit, &id, bond_current)?
                        .ok_or(invalid("a pricing rule names a stored bond"))?;
                    if !rule.applies_to(&bond) {
                        return Err(invalid(RULE_DOES_NOT_APPLY));
                    }
                    if rule.width.is_some() && !is_quoted_two_way(&bond) {
                        return Err(invalid(WIDTH_NEEDS_TWO_WAY));
                    }
                }
                if let Some(set) = &rule.benchmark_set_id {
                    ensure_master_exists(commit, &MasterKey::BenchmarkSet(set.clone()), master)?;
                }
            }
            MasterDefinition::BenchmarkSet(set) => {
                for security in &set.securities {
                    let pin = InstrumentPin {
                        id: security.instrument_id.clone(),
                        version: security.version,
                    };
                    let bond = exact_bond(commit, &pin, bond_version)?;
                    if !is_benchmark_bond(&bond, set.currency) {
                        return Err(invalid(
                            "a benchmark security is a government fixed bullet in the set's currency",
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Why a rule does not apply to a bond ([`PricingMethod::applies_to`]).
pub const RULE_DOES_NOT_APPLY: &str = "only a fixed bullet is quoted by a spread or against a benchmark set, and by a yield only a fixed bullet or a callable valued on its workouts";

/// Why a callable is not valued on its workouts ([`is_workout_callable`]).
pub const NOT_A_WORKOUT_CALLABLE: &str = "a callable is valued on its workouts only with European or Bermudan fixed-price calls, at most 128, and no other embedded terms";

/// A fixed callable the workout model values: European or Bermudan fixed-price calls, at
/// most [`MAX_WORKOUT_CALLS`], and nothing beyond its calls
/// ([`BondReferenceData::has_terms_beyond_calls`]).
pub fn is_workout_callable(bond: &BondReferenceData) -> bool {
    bond.bond_type == BondType::FixedCallable
        && bond.is_callable
        && (1..=MAX_WORKOUT_CALLS).contains(&bond.call_schedule.len())
        && !bond.call_schedule.iter().any(|call| call.is_make_whole)
        && !bond.has_terms_beyond_calls()
        && bond
            .contractual_terms
            .as_ref()
            .and_then(|terms| terms.call_terms.as_ref())
            .is_some_and(|call| call.style != CallStyle::American)
}

/// A fixed-coupon bullet with no embedded terms: what the router values and the level
/// stage prices.
pub fn is_plain_bullet(bond: &BondReferenceData) -> bool {
    bond.bond_type == BondType::FixedBullet && !bond.has_embedded_terms()
}

/// A fixed bullet or a zero with no embedded terms: what the pricing router values and
/// quotes two-way, so what a rule's width applies to. Callables and floating rate notes
/// are quoted mid only.
pub fn is_quoted_two_way(bond: &BondReferenceData) -> bool {
    matches!(bond.bond_type, BondType::FixedBullet | BondType::ZeroCoupon)
        && !bond.has_embedded_terms()
}

/// Why a rule's width is refused for a bond: only a bond quoted two-way takes one.
pub const WIDTH_NEEDS_TWO_WAY: &str =
    "a width applies only to a fixed bullet or a zero, the bonds quoted two-way";

/// A security a benchmark set may hold: a sovereign's plain bullet in the set's currency.
fn is_benchmark_bond(bond: &BondReferenceData, currency: Currency) -> bool {
    is_plain_bullet(bond) && bond.issuer_type == IssuerType::Sovereign && bond.currency == currency
}

/// The months a tenor names: a whole number of months (`M`) or years (`Y`) with no
/// leading zero, from 1 month to [`MAX_BENCHMARK_TENOR_MONTHS`].
pub fn tenor_months(tenor: &str) -> Result<u32, ReferenceError> {
    let refused =
        || invalid("a benchmark tenor is months or years such as 6M or 10Y, at most 50 years");
    let (number, per) = match (tenor.strip_suffix('M'), tenor.strip_suffix('Y')) {
        (Some(number), _) => (number, 1),
        (_, Some(number)) => (number, 12),
        _ => return Err(refused()),
    };
    if number.is_empty()
        || number.len() > 3
        || number.starts_with('0')
        || !number.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(refused());
    }
    let months = number
        .parse::<u32>()
        .ok()
        .and_then(|n| n.checked_mul(per))
        .ok_or_else(refused)?;
    if months > MAX_BENCHMARK_TENOR_MONTHS {
        return Err(refused());
    }
    Ok(months)
}

fn invalid(reason: &'static str) -> ReferenceError {
    ReferenceError::InvalidInput(reason)
}
