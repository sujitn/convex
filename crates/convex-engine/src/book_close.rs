//! Desk closes of a book, and the P&L since one.
//!
//! [`position_value`] values one position as the limit evaluator admits its quote
//! (`portfolio_analytics::quoted_prices`): the book's currency, face quantities, a
//! non-stale quote with a bounded lineage and non-negative prices. A close records it for
//! each position, and the P&L reads it again now.
//!
//! [`holdings_since_close`] lines a close up against the book now, a pure function of
//! both. The caller supplies the entitled cash of each holding that
//! [`HoldingSinceClose::needs_entitled_cash`], from
//! [`PricingEngine::entitled_cash`](crate::PricingEngine::entitled_cash), and sums them with
//! `convex_portfolio::pnl_since_close`.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::HashMap;

use convex::portfolio::pnl_since_close::HoldingSinceClose;
use convex_core::contracts::{ExactDecimal, InstrumentId, UtcTimestamp, Version};
use convex_core::{Currency, Date};
use convex_math::decimal::BoundedDecimal;
use rust_decimal::Decimal;

use crate::error::EngineError;
use crate::floating_valuation::FloatingNote;
use crate::limits::detail;
use crate::portfolio_analytics::quoted_prices;
use crate::ports::book_close::{BookCloseDefinition, PositionValue, UnvaluedReason};
use crate::ports::output::BondQuoteOutput;
use crate::ports::reference_data::{BondReferenceData, BondType};
use crate::ports::reference_master::{
    is_valued_floating, QuantityUnit, ReferenceLookup, MAX_BOOK_POSITIONS,
};
use crate::pricing_router::PricingRouter;

/// A position's accepted Live result read as a quote, with the instrument version it
/// priced.
#[derive(Debug, Clone, Copy)]
pub struct PricedQuote<'q> {
    /// The quote.
    pub quote: &'q BondQuoteOutput,
    /// The instrument version it priced.
    pub instrument_version: Version,
}

/// One position's value from its `priced` quote, checked in the limit evaluator's order:
/// a face quantity, a result, the book's `currency`, then the quote's prices.
#[must_use]
pub fn position_value(
    instrument_id: &InstrumentId,
    unit: QuantityUnit,
    priced: Option<PricedQuote<'_>>,
    currency: Currency,
) -> PositionValue {
    let unvalued = |reason| PositionValue::Unvalued { reason };
    if unit != QuantityUnit::Face {
        return unvalued(UnvaluedReason::UnsupportedQuantityUnit {});
    }
    let Some(PricedQuote {
        quote,
        instrument_version,
    }) = priced
    else {
        return unvalued(UnvaluedReason::MissingResult {});
    };
    if quote.currency != currency {
        return unvalued(UnvaluedReason::ForeignCurrency {});
    }
    match quoted_prices(instrument_id.as_str(), quote) {
        Ok(prices) => PositionValue::Valued {
            instrument_version,
            clean_price_mid: ExactDecimal::new(prices.clean),
            accrued_interest: ExactDecimal::new(prices.accrued),
        },
        Err(error) => unvalued(UnvaluedReason::QuoteRefused {
            detail: detail(&error),
        }),
    }
}

/// A value's dirty price per 100 face, clean plus accrued rounded nearest/even to what a
/// `Decimal` holds: the one rounding both the close and now go through. `None` when
/// unvalued.
pub fn dirty_price(value: &PositionValue) -> Result<Option<Decimal>, EngineError> {
    let PositionValue::Valued {
        clean_price_mid,
        accrued_interest,
        ..
    } = value
    else {
        return Ok(None);
    };
    BoundedDecimal::rounded_sum(clean_price_mid.decimal(), accrued_interest.decimal())
        .map(Some)
        .map_err(|error| EngineError::PricingError(format!("dirty price: {error}")))
}

/// One position of the book now, with its value.
#[derive(Debug, Clone, Copy)]
pub struct CurrentPosition<'a> {
    /// The instrument.
    pub instrument_id: &'a InstrumentId,
    /// Signed quantity.
    pub quantity: Decimal,
    /// Its value now.
    pub value: &'a PositionValue,
}

/// Each instrument held at the close or now, as the P&L since the close reads it: the
/// close's positions in its order, then those added since in the book's. An instrument
/// held at neither (a zero quantity at both) is left out. Entitled cash is `None`: the
/// caller fills it for each holding that needs it.
///
/// # Errors
///
/// An instrument given twice now, more than `MAX_BOOK_POSITIONS` positions now, or a
/// dirty price a `Decimal` cannot hold.
pub fn holdings_since_close<'a>(
    close: &'a BookCloseDefinition,
    now: &[CurrentPosition<'a>],
) -> Result<Vec<HoldingSinceClose<'a>>, EngineError> {
    if now.len() > MAX_BOOK_POSITIONS {
        return Err(EngineError::PricingError(format!(
            "a book holds at most {MAX_BOOK_POSITIONS} positions"
        )));
    }
    let mut by_instrument: HashMap<&str, &CurrentPosition<'a>> = HashMap::with_capacity(now.len());
    for position in now {
        if by_instrument
            .insert(position.instrument_id.as_str(), position)
            .is_some()
        {
            return Err(EngineError::PricingError(format!(
                "instrument {} is held twice",
                position.instrument_id
            )));
        }
    }
    let mut holdings = Vec::with_capacity(close.positions.len() + now.len());
    for position in &close.positions {
        let current = by_instrument.remove(position.instrument_id.as_str());
        let holding = HoldingSinceClose {
            instrument_id: position.instrument_id.as_str(),
            close_face: position.quantity.decimal(),
            close_dirty_price: dirty_price(&position.value)?,
            face: current.map_or(Decimal::ZERO, |p| p.quantity),
            dirty_price: match current {
                Some(p) => dirty_price(p.value)?,
                None => None,
            },
            entitled_cash: None,
        };
        if !(holding.close_face.is_zero() && holding.face.is_zero()) {
            holdings.push(holding);
        }
    }
    for position in now {
        // Each instrument the close holds was taken from the map above.
        if !by_instrument.contains_key(position.instrument_id.as_str())
            || position.quantity.is_zero()
        {
            continue;
        }
        holdings.push(HoldingSinceClose {
            instrument_id: position.instrument_id.as_str(),
            close_face: Decimal::ZERO,
            close_dirty_price: None,
            face: position.quantity,
            dirty_price: dirty_price(position.value)?,
            entitled_cash: None,
        });
    }
    Ok(holdings)
}

impl crate::PricingEngine {
    /// The cash per 100 face a holder of one priced bond version settling at `from` is
    /// owed and one settling at `to` is not: `None` when it cannot be projected. `bond` is
    /// the version's terms.
    ///
    /// A fixed bullet or callable reads its bullet schedule, and a zero its redemption, as
    /// the router builds them, so the cash comes from the schedule its accrued reads. A
    /// valued floating rate note reads its contract, built with its pinned calendars and
    /// rate index as its valuation builds it: its cash is not projected when a coupon
    /// falls between the two dates. Any other bond is not projected. Nothing is read or
    /// built when the dates are equal.
    ///
    /// # Errors
    ///
    /// `to` before `from`, terms the router refuses to build, or a floater whose pinned
    /// resources cannot be read.
    pub async fn entitled_cash(
        &self,
        version: Version,
        bond: &BondReferenceData,
        from: Date,
        to: Date,
    ) -> Result<Option<Decimal>, EngineError> {
        let note = if to > from && is_valued_floating(bond) {
            Some(self.floating_note(&bond.instrument_id, version).await?)
        } else {
            None
        };
        cash_between(bond, note.as_ref(), from, to)
    }

    /// A floating rate note's version, resolved with its pinned resources.
    async fn floating_note(
        &self,
        instrument_id: &convex_core::ids::InstrumentId,
        version: Version,
    ) -> Result<FloatingNote, EngineError> {
        let refused =
            |reason: String| EngineError::PricingError(format!("entitled cash: {reason}"));
        let repo = self.reference_repository();
        let record = repo
            .get_version(instrument_id, version)
            .await
            .map_err(|error| refused(error.to_string()))?
            .ok_or_else(|| refused("the priced version is missing".into()))?;
        let now = UtcTimestamp::from_datetime(chrono::Utc::now())
            .map_err(|_| refused("the clock is outside the supported range".into()))?;
        let lookup = ReferenceLookup {
            as_of: now,
            known_at: now,
        };
        let resolved = repo
            .resolve_pinned(record, lookup)
            .await
            .map_err(|error| refused(error.to_string()))?;
        FloatingNote::new(&resolved).map_err(|failure| refused(failure.message))
    }
}

/// [`crate::PricingEngine::entitled_cash`] once a valued floater's note is built.
fn cash_between(
    bond: &BondReferenceData,
    floating: Option<&FloatingNote>,
    from: Date,
    to: Date,
) -> Result<Option<Decimal>, EngineError> {
    if to < from {
        return Err(EngineError::PricingError(
            "entitled cash: the later settlement date is before the earlier".into(),
        ));
    }
    if to == from {
        return Ok(Some(Decimal::ZERO));
    }
    let router = PricingRouter::new();
    let projected = |result: convex_bonds::BondResult<Decimal>| {
        result
            .map(Some)
            .map_err(|error| EngineError::PricingError(format!("entitled cash: {error}")))
    };
    match bond.bond_type {
        BondType::FixedBullet | BondType::FixedCallable => projected(
            convex_bonds::cashflows::entitled_cash(&router.to_fixed_rate_bond(bond)?, from, to),
        ),
        BondType::ZeroCoupon => projected(convex_bonds::cashflows::entitled_cash(
            &router.to_zero_coupon_bond(bond)?,
            from,
            to,
        )),
        BondType::FloatingRate => match floating.map(|note| note.entitled_cash(from, to)) {
            Some(Ok(cash)) => Ok(Some(cash)),
            Some(Err(convex_bonds::contracts::BondContractError::Unsupported { .. })) | None => {
                Ok(None)
            }
            Some(Err(error)) => Err(EngineError::PricingError(format!("entitled cash: {error}"))),
        },
        BondType::FixedPutable
        | BondType::InflationLinked
        | BondType::Amortizing
        | BondType::Convertible => Ok(None),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ports::book_close::{BookCloseDefinition, CloseBasis, CloseTaker, ClosedPosition};
    use crate::ports::reference_master::{EffectiveInterval, ReferenceProvenance};
    use convex_core::contracts::{BusinessDate, ReferenceDataId, SnapshotId, SourceId};
    use rust_decimal_macros::dec;

    fn bond() -> BondReferenceData {
        serde_json::from_str(include_str!(
            "../../convex-ports/tests/fixtures/reference_bond.json"
        ))
        .unwrap()
    }

    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    fn quote(clean: Decimal, accrued: Decimal) -> BondQuoteOutput {
        BondQuoteOutput {
            clean_price_mid: Some(clean),
            accrued_interest: Some(accrued),
            ..BondQuoteOutput::without_measures(
                convex_core::ids::InstrumentId::new("BOND-001"),
                Currency::USD,
                date("2026-10-06"),
                1,
                "spec".into(),
                "source".into(),
            )
        }
    }

    fn id(text: &str) -> InstrumentId {
        InstrumentId::new(text).unwrap()
    }

    fn priced(quote: &BondQuoteOutput) -> Option<PricedQuote<'_>> {
        Some(PricedQuote {
            quote,
            instrument_version: Version::new(3).unwrap(),
        })
    }

    #[test]
    fn a_position_is_valued_as_the_limit_evaluator_admits_its_quote() {
        let version = Version::new(3).unwrap();
        let read = quote(dec!(99.25), dec!(1.1956521739130434));
        assert_eq!(
            position_value(&id("A"), QuantityUnit::Face, priced(&read), Currency::USD),
            PositionValue::Valued {
                instrument_version: version,
                clean_price_mid: ExactDecimal::new(dec!(99.25)),
                accrued_interest: ExactDecimal::new(dec!(1.1956521739130434)),
            }
        );
        let reason = |value| match value {
            PositionValue::Unvalued { reason } => reason,
            valued => panic!("valued: {valued:?}"),
        };
        assert_eq!(
            reason(position_value(
                &id("A"),
                QuantityUnit::Units,
                priced(&read),
                Currency::USD
            )),
            UnvaluedReason::UnsupportedQuantityUnit {}
        );
        assert_eq!(
            reason(position_value(
                &id("A"),
                QuantityUnit::Face,
                None,
                Currency::USD
            )),
            UnvaluedReason::MissingResult {}
        );
        assert_eq!(
            reason(position_value(
                &id("A"),
                QuantityUnit::Face,
                priced(&read),
                Currency::EUR
            )),
            UnvaluedReason::ForeignCurrency {}
        );
        let stale = BondQuoteOutput {
            is_stale: true,
            ..read.clone()
        };
        assert!(matches!(
            reason(position_value(
                &id("A"),
                QuantityUnit::Face,
                priced(&stale),
                Currency::USD
            )),
            UnvaluedReason::QuoteRefused { .. }
        ));
    }

    #[test]
    fn a_dirty_price_is_rounded_once_to_what_a_decimal_holds() {
        let value = |clean, accrued| PositionValue::Valued {
            instrument_version: Version::new(1).unwrap(),
            clean_price_mid: ExactDecimal::new(clean),
            accrued_interest: ExactDecimal::new(accrued),
        };
        assert_eq!(
            dirty_price(&value(dec!(99.25), dec!(1.25))).unwrap(),
            Some(dec!(100.50))
        );
        // 101.25 plus 28 places of accrued has 31 digits: the 29th place rounds half to even.
        assert_eq!(
            dirty_price(&value(dec!(101.25), dec!(0.5138888888888888888888888889)))
                .unwrap()
                .unwrap()
                .to_string(),
            "101.76388888888888888888888889"
        );
    }

    fn close(positions: Vec<(&str, Decimal, PositionValue)>) -> BookCloseDefinition {
        BookCloseDefinition {
            book_id: ReferenceDataId::new("DESK").unwrap(),
            close_date: BusinessDate::parse("2026-10-05").unwrap(),
            basis: CloseBasis::DeskClose,
            taken_by: CloseTaker::Schedule {},
            book_version: Version::new(1).unwrap(),
            currency: "USD".into(),
            settlement_date: BusinessDate::parse("2026-10-06").unwrap(),
            snapshot_ids: vec![SnapshotId::new("snapshot-1").unwrap()],
            positions: positions
                .into_iter()
                .map(|(instrument, quantity, value)| ClosedPosition {
                    instrument_id: id(instrument),
                    quantity: ExactDecimal::new(quantity),
                    unit: QuantityUnit::Face,
                    value,
                })
                .collect(),
            effective: EffectiveInterval {
                from: UtcTimestamp::parse("2026-10-05T21:30:00Z").unwrap(),
                until: None,
            },
            provenance: ReferenceProvenance {
                source: SourceId::new("book-close").unwrap(),
                evidence_reference: "book-close:test".into(),
            },
        }
    }

    #[test]
    fn holdings_line_the_close_up_against_the_book_now() {
        let valued = PositionValue::Valued {
            instrument_version: Version::new(1).unwrap(),
            clean_price_mid: ExactDecimal::new(dec!(99)),
            accrued_interest: ExactDecimal::new(dec!(1)),
        };
        let missing = PositionValue::Unvalued {
            reason: UnvaluedReason::MissingResult {},
        };
        let closed = close(vec![
            ("HELD", dec!(1000), valued.clone()),
            ("REMOVED", dec!(500), valued.clone()),
            ("ZERO", Decimal::ZERO, valued.clone()),
            ("UNVALUED", dec!(200), missing.clone()),
        ]);
        let (held, added, unvalued, zero) = (id("HELD"), id("ADDED"), id("UNVALUED"), id("NONE"));
        let now = [
            CurrentPosition {
                instrument_id: &added,
                quantity: dec!(300),
                value: &valued,
            },
            CurrentPosition {
                instrument_id: &held,
                quantity: dec!(1500),
                value: &valued,
            },
            CurrentPosition {
                instrument_id: &unvalued,
                quantity: dec!(200),
                value: &valued,
            },
            CurrentPosition {
                instrument_id: &zero,
                quantity: Decimal::ZERO,
                value: &valued,
            },
        ];
        let holdings = holdings_since_close(&closed, &now).unwrap();
        let rows: Vec<_> = holdings
            .iter()
            .map(|h| {
                (
                    h.instrument_id,
                    h.close_face,
                    h.close_dirty_price,
                    h.face,
                    h.dirty_price,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "HELD",
                    dec!(1000),
                    Some(dec!(100)),
                    dec!(1500),
                    Some(dec!(100))
                ),
                ("REMOVED", dec!(500), Some(dec!(100)), Decimal::ZERO, None),
                ("UNVALUED", dec!(200), None, dec!(200), Some(dec!(100))),
                ("ADDED", Decimal::ZERO, None, dec!(300), Some(dec!(100))),
            ]
        );
        assert!(holdings.iter().all(|h| h.entitled_cash.is_none()));
        let needing: Vec<_> = holdings
            .iter()
            .filter(|h| h.needs_entitled_cash())
            .map(|h| h.instrument_id)
            .collect();
        assert_eq!(needing, ["HELD"]);
        let twice = [now[1], now[1]];
        assert!(holdings_since_close(&closed, &twice).is_err());
    }

    #[test]
    fn cash_is_projected_from_the_schedule_the_router_builds() {
        let bond = bond();
        // 5.375% ACT/ACT: the 15 January 2027 coupon is on 184 days of a 365-day year.
        let coupon = cash_between(&bond, None, date("2026-10-06"), date("2027-01-19"))
            .unwrap()
            .unwrap();
        assert!((coupon - dec!(5.375) * dec!(184) / dec!(365)).abs() < dec!(0.000000000001));
        assert_eq!(
            cash_between(&bond, None, date("2026-10-06"), date("2026-10-06")).unwrap(),
            Some(Decimal::ZERO)
        );
        assert!(cash_between(&bond, None, date("2026-10-07"), date("2026-10-06")).is_err());
        let floater = BondReferenceData {
            bond_type: BondType::FloatingRate,
            ..bond.clone()
        };
        // A floater whose note was not built is not projected.
        assert_eq!(
            cash_between(&floater, None, date("2026-10-06"), date("2027-01-19")).unwrap(),
            None
        );
        let amortizing = BondReferenceData {
            bond_type: BondType::Amortizing,
            ..bond
        };
        assert_eq!(
            cash_between(&amortizing, None, date("2026-10-06"), date("2027-01-19")).unwrap(),
            None
        );
    }
}
