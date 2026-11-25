//! P&L since a desk close.
//!
//! A close records each position of a book with its face and its dirty price per 100 at
//! the close's settlement date. The P&L since the close of a position is on the face held
//! through, the smaller of the two sizes on the side held at both:
//!
//! `face held × (dirty price now − dirty price at the close + entitled cash) / 100`
//!
//! A value is face × dirty price / 100, the net dirty value the limit evaluator reads, so
//! the change in value is the one a limit on net market value sees. The entitled cash is
//! what a holder became owed between the close's settlement date and now's: each coupon,
//! sink or redemption paid in between or gone ex-coupon
//! ([`convex_bonds::cashflows::entitled_cash`], or
//! [`BondContract::entitled_cash`](convex_bonds::contracts::BondContract::entitled_cash)
//! for a floating rate note). A dirty price drops by a coupon as it goes ex, so without the
//! cash a coupon date would show the coupon as a loss.
//!
//! No trade price is known (there is no trade capture), so a position added since the
//! close, a position removed, one now on the other side, and the face added or removed
//! beyond the face held through have no P&L. A position the close or now could not value,
//! or whose cash could not be projected, has none either. Each says why, and the total
//! leaves it out.
//!
//! The sums are exact through `convex-math` `BoundedDecimal`; only each reported amount is
//! rounded, nearest/even, to what a `Decimal` holds. An amount that is too large for a
//! `Decimal`, or not zero but too small to show as one, is refused rather than erased.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cmp;
use std::collections::HashSet;

use convex_math::decimal::BoundedDecimal;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{PortfolioError, PortfolioResult};

/// One instrument as the P&L since the close reads it: as the close recorded it, and as
/// held now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldingSinceClose<'a> {
    /// The instrument, unique in a book.
    pub instrument_id: &'a str,
    /// Signed face at the close; zero when the close did not hold it.
    pub close_face: Decimal,
    /// Dirty price per 100 at the close; `None` when the close could not value it.
    pub close_dirty_price: Option<Decimal>,
    /// Signed face now; zero when it is no longer held.
    pub face: Decimal,
    /// Dirty price per 100 now; `None` when it has no value now.
    pub dirty_price: Option<Decimal>,
    /// Cash per 100 face entitled between the close's settlement date and now's;
    /// `None` when it could not be projected.
    pub entitled_cash: Option<Decimal>,
}

impl HoldingSinceClose<'_> {
    /// Whether its P&L reads entitled cash: it is held through the close on one side,
    /// and valued at the close and now. Only then need the cash be projected.
    #[must_use]
    pub fn needs_entitled_cash(&self) -> bool {
        held_through(&HoldingSinceClose {
            entitled_cash: Some(Decimal::ZERO),
            ..*self
        })
        .is_ok()
    }
}

/// Why a position has no P&L since the close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoPnlReason {
    /// Not held at the close; it has no trade price.
    AddedSinceClose,
    /// No longer held; it has no trade price.
    RemovedSinceClose,
    /// Held now on the other side from the close.
    SideChanged,
    /// The close could not value it.
    UnvaluedAtClose,
    /// It has no value now.
    UnvaluedNow,
    /// Its cash between the two settlement dates could not be projected.
    CashNotProjected,
}

/// The P&L of a position held through the close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldPnl {
    /// Signed face held through: the smaller of the two sizes, on the side held.
    pub held_face: Decimal,
    /// The P&L on the face held through, entitled cash included.
    pub pnl: Decimal,
    /// Of which entitled cash.
    pub entitled_cash: Decimal,
}

/// One position's P&L since the close, in the order given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionPnl {
    /// The instrument.
    pub instrument_id: String,
    /// Its P&L on the face held through, or why it has none.
    pub held: Result<HeldPnl, NoPnlReason>,
}

/// A book's P&L since its close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PnlSinceClose {
    /// Each position, in the order given.
    pub positions: Vec<PositionPnl>,
    /// The P&L of every position that has one, entitled cash included.
    pub pnl: Decimal,
    /// Of which entitled cash.
    pub entitled_cash: Decimal,
}

impl PnlSinceClose {
    /// The positions with no P&L, which the total leaves out.
    #[must_use]
    pub fn excluded_count(&self) -> usize {
        self.positions
            .iter()
            .filter(|position| position.held.is_err())
            .count()
    }
}

/// The P&L since the close of each holding, and their total.
///
/// # Errors
///
/// [`PortfolioError::InvalidHolding`] for an instrument given twice, one held neither at
/// the close nor now, a negative dirty price, negative entitled cash, or an amount a
/// `Decimal` cannot hold; [`PortfolioError::InvalidPortfolio`] for a total it cannot hold.
pub fn pnl_since_close(holdings: &[HoldingSinceClose<'_>]) -> PortfolioResult<PnlSinceClose> {
    let mut positions = Vec::with_capacity(holdings.len());
    let total = sum_held(holdings, |id, held| {
        let held = match held {
            Ok(held) => Ok(HeldPnl {
                held_face: held.held_face,
                pnl: finish(&held.pnl).map_err(|_| amount_overflow(id))?,
                entitled_cash: finish(&held.entitled_cash).map_err(|_| amount_overflow(id))?,
            }),
            Err(reason) => Err(reason),
        };
        positions.push(PositionPnl {
            instrument_id: id.to_owned(),
            held,
        });
        Ok(())
    })?;
    Ok(PnlSinceClose {
        positions,
        pnl: total.pnl,
        entitled_cash: total.entitled_cash,
    })
}

/// A book's P&L since its close, as its total alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PnlTotalSinceClose {
    /// The P&L of every position that has one, entitled cash included.
    pub pnl: Decimal,
    /// Of which entitled cash.
    pub entitled_cash: Decimal,
    /// The positions with no P&L, which the total leaves out.
    pub excluded_count: usize,
}

/// The total of [`pnl_since_close`] without each position's amounts: the same exact sums,
/// rounded once, which skips rounding each position's.
///
/// # Errors
///
/// As [`pnl_since_close`], except that a position's own amount is not rounded, so one a
/// `Decimal` cannot hold refuses only the listing.
pub fn pnl_total_since_close(
    holdings: &[HoldingSinceClose<'_>],
) -> PortfolioResult<PnlTotalSinceClose> {
    sum_held(holdings, |_, _| Ok(()))
}

/// A position's exact P&L on the face held through, and the cash's part.
struct ExactHeldPnl {
    held_face: Decimal,
    pnl: BoundedDecimal,
    entitled_cash: BoundedDecimal,
}

/// Check each holding, in order, and total the P&L of those held through; `each` is given
/// every holding's exact P&L, or why it has none.
fn sum_held(
    holdings: &[HoldingSinceClose<'_>],
    mut each: impl FnMut(&str, Result<ExactHeldPnl, NoPnlReason>) -> PortfolioResult<()>,
) -> PortfolioResult<PnlTotalSinceClose> {
    let mut seen = HashSet::with_capacity(holdings.len());
    let mut total = BoundedDecimal::zero();
    let mut total_cash = BoundedDecimal::zero();
    let mut excluded_count = 0;
    for holding in holdings {
        let id = holding.instrument_id;
        if !seen.insert(id) {
            return Err(PortfolioError::invalid_holding(id, "given twice"));
        }
        check_holding(holding)?;
        let held = match held_through(holding) {
            Ok(held) => {
                let (pnl, cash) = held_pnl(id, &held)?;
                total.add(&pnl).map_err(|_| amount_overflow(id))?;
                total_cash.add(&cash).map_err(|_| amount_overflow(id))?;
                Ok(ExactHeldPnl {
                    held_face: held.held_face,
                    pnl,
                    entitled_cash: cash,
                })
            }
            Err(reason) => Err(reason),
        };
        excluded_count += usize::from(held.is_err());
        each(id, held)?;
    }
    let total_overflow = |_| PortfolioError::invalid_portfolio(OVERFLOW);
    Ok(PnlTotalSinceClose {
        pnl: finish(&total).map_err(total_overflow)?,
        entitled_cash: finish(&total_cash).map_err(total_overflow)?,
        excluded_count,
    })
}

const OVERFLOW: &str = "an amount is too large, or too small, for a Decimal";

/// A position held through the close, with the prices and cash its P&L reads.
struct HeldThrough {
    held_face: Decimal,
    close_dirty_price: Decimal,
    dirty_price: Decimal,
    entitled_cash: Decimal,
}

/// Refuses a holding no book can have: held at neither date, or with a negative price
/// or negative cash.
fn check_holding(holding: &HoldingSinceClose<'_>) -> PortfolioResult<()> {
    let refuse = |reason| {
        Err(PortfolioError::invalid_holding(
            holding.instrument_id,
            reason,
        ))
    };
    if holding.close_face.is_zero() && holding.face.is_zero() {
        return refuse("held neither at the close nor now");
    }
    let is_negative = |amount: Option<Decimal>| amount.is_some_and(|a| a < Decimal::ZERO);
    if is_negative(holding.close_dirty_price) || is_negative(holding.dirty_price) {
        return refuse("a dirty price is negative");
    }
    if is_negative(holding.entitled_cash) {
        return refuse("entitled cash is negative");
    }
    Ok(())
}

/// The face held through and what its P&L reads, or why it has no P&L.
fn held_through(holding: &HoldingSinceClose<'_>) -> Result<HeldThrough, NoPnlReason> {
    let (close_face, face) = (holding.close_face, holding.face);
    if close_face.is_zero() {
        return Err(NoPnlReason::AddedSinceClose);
    }
    if face.is_zero() {
        return Err(NoPnlReason::RemovedSinceClose);
    }
    if close_face.is_sign_negative() != face.is_sign_negative() {
        return Err(NoPnlReason::SideChanged);
    }
    Ok(HeldThrough {
        held_face: cmp::min_by_key(close_face, face, |f| f.abs()),
        close_dirty_price: holding
            .close_dirty_price
            .ok_or(NoPnlReason::UnvaluedAtClose)?,
        dirty_price: holding.dirty_price.ok_or(NoPnlReason::UnvaluedNow)?,
        entitled_cash: holding.entitled_cash.ok_or(NoPnlReason::CashNotProjected)?,
    })
}

/// `face × (dirty price − dirty price at the close + cash) / 100`, and the cash's part.
fn held_pnl(id: &str, held: &HeldThrough) -> PortfolioResult<(BoundedDecimal, BoundedDecimal)> {
    let on_face = |per_100: &BoundedDecimal| {
        per_100
            .multiply(held.held_face)
            .and_then(|amount| amount.divide_power_of_ten(2))
            .map_err(|_| amount_overflow(id))
    };
    let cash = BoundedDecimal::decimal(held.entitled_cash);
    let mut gain = BoundedDecimal::decimal(held.dirty_price);
    gain.add(&BoundedDecimal::decimal(-held.close_dirty_price))
        .map_err(|_| amount_overflow(id))?;
    gain.add(&cash).map_err(|_| amount_overflow(id))?;
    Ok((on_face(&gain)?, on_face(&cash)?))
}

fn finish(amount: &BoundedDecimal) -> convex_math::MathResult<Decimal> {
    amount.finish().map(|amount| amount.normalize())
}

fn amount_overflow(id: &str) -> PortfolioError {
    PortfolioError::invalid_holding(id, OVERFLOW)
}
