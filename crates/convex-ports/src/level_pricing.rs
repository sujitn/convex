//! Pricing rules and benchmark sets (P19, D111–D113): how a stored bond is quoted, and the
//! government securities a spread is quoted to.
//!
//! A bond's [`PricingRuleDefinition`] (`MasterKey::PricingRule`) names its method: a price,
//! a yield, a spread to one benchmark security, or a Z-spread. The level itself is a quote,
//! never stored (D111). A rule may name a [`BenchmarkSetDefinition`]
//! (`MasterKey::BenchmarkSet`) by id, so an on-the-run roll is one commit to the set and
//! the version a valuation read is recorded with its result. A bond with no rule is
//! quoted by price.
//!
//! These are types, bounds and validation only. Turning a level into a price is the
//! engine's level stage, through the pure crates.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeSet;

use convex_core::contracts::{InstrumentId, ReferenceDataId, Version};
use convex_core::futures::InstrumentPin;
use convex_core::Currency;
use serde::{Deserialize, Serialize};

use crate::futures_reference::exact_bond;
use crate::reference_data::{BondReferenceData, BondType, IssuerType};
use crate::reference_master::{
    bond_at_commit, ensure_master_exists, EffectiveInterval, MasterDefinition, MasterKey,
    MasterRecord, ReferenceProvenance,
};
use crate::reference_repository::{ReferenceCommit, ReferenceError, ReferenceRecord};

/// Securities in one benchmark set (D113).
pub const MAX_BENCHMARK_SECURITIES: usize = 32;
/// The longest benchmark tenor, in months: 50 years.
pub const MAX_BENCHMARK_TENOR_MONTHS: u32 = 600;

/// How a stored bond is quoted. Each method other than `price` is a level that the level
/// stage turns into a price before the bond's model values it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PricingMethod {
    /// A clean price per 100 face, as a bond with no rule is quoted.
    Price {},
    /// A street yield to maturity, at the bond's coupon frequency.
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
}

/// How one stored bond is quoted (D111, D112). Versioned; changed rarely. The level a
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
    /// Validity; a bounded end retires the rule, and the bond is quoted by price again.
    pub effective: EffectiveInterval,
    /// Source evidence.
    pub provenance: ReferenceProvenance,
}

impl PricingRuleDefinition {
    /// Whether the rule quotes a level or names a benchmark set, either of which only a
    /// plain fixed bullet takes.
    pub fn requires_plain_bullet(&self) -> bool {
        self.method.is_level() || self.benchmark_set_id.is_some()
    }

    /// Validate the rule on its own: a benchmark spread names its set and a well-formed
    /// tenor. Whether the bond takes the method is checked against the stored bond.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        match &self.method {
            PricingMethod::BenchmarkSpread { tenor } => {
                if self.benchmark_set_id.is_none() {
                    return Err(invalid("a benchmark spread names its benchmark set"));
                }
                tenor_months(tenor).map(|_| ())
            }
            PricingMethod::Price {} | PricingMethod::Yield {} | PricingMethod::ZSpread {} => Ok(()),
        }
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

/// A versioned set of government securities that spreads are quoted to (D113). Rolling a
/// benchmark to a new issue is one commit of a new version. P19 quotes against US
/// Treasuries, so a set is in USD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSetDefinition {
    /// The set's identity, such as `UST-OTR`.
    pub benchmark_set_id: ReferenceDataId,
    /// The currency of every security in it.
    pub currency: Currency,
    /// 1 to [`MAX_BENCHMARK_SECURITIES`] securities, with unique tenors and instruments.
    pub securities: Vec<BenchmarkSecurity>,
    /// Validity; a bounded end retires the set.
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
                "a benchmark set is in USD: P19 quotes against Treasuries",
            ));
        }
        if self.securities.is_empty() {
            return Err(invalid("a benchmark set holds at least one security"));
        }
        if self.securities.len() > MAX_BENCHMARK_SECURITIES {
            return Err(ReferenceError::CapacityExceeded);
        }
        let mut tenors = BTreeSet::new();
        let mut instruments = BTreeSet::new();
        for security in &self.securities {
            if !tenors.insert(tenor_months(&security.tenor)?) {
                return Err(invalid("duplicate benchmark tenor"));
            }
            if !instruments.insert(security.instrument_id.as_str()) {
                return Err(invalid("duplicate benchmark security"));
            }
        }
        Ok(())
    }
}

/// Check a commit's rules and sets against the bonds and sets they name, each as of the
/// commit: a rule's bond is stored, and quoted by a level only if it is a fixed bullet
/// (P19a; floating rate notes and callables follow in P19c); a named set exists; each
/// security of a set is a stored government fixed bullet in the set's currency.
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
                let id: convex_core::ids::InstrumentId = rule.instrument_id.clone().into();
                let bond = bond_at_commit(commit, &id, bond_current)?
                    .ok_or(invalid("a pricing rule names a stored bond"))?;
                if rule.requires_plain_bullet() && !is_plain_bullet(&bond) {
                    return Err(invalid(
                        "only a fixed bullet is quoted by a level or against a benchmark set",
                    ));
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

/// A fixed-coupon bullet with no embedded terms: what the router values and the level
/// stage prices.
pub fn is_plain_bullet(bond: &BondReferenceData) -> bool {
    bond.bond_type == BondType::FixedBullet && !bond.has_embedded_terms()
}

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
