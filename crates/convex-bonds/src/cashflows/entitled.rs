//! The cash a holder becomes entitled to between two settlement dates.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_core::Date;
use convex_math::decimal::BoundedDecimal;
use convex_math::MathError;
use rust_decimal::Decimal;

use crate::error::{BondError, BondResult};
use crate::traits::Bond;

/// The cash per 100 face that a holder settling at `from` is owed and one settling at `to`
/// is not: each coupon, sink or redemption paid in between or gone ex-coupon.
///
/// It is the bond's cash flows from `from` less those from `to`, from the schedule the
/// bond's accrued interest reads, so a coupon that leaves the dirty price comes back as
/// cash. A coupon already ex at `from` is owed to neither, so it is not counted; one that
/// goes ex in between is, though it is paid later. When the two dates are the same the
/// cash is zero and nothing is projected. Amounts are per 100 of [`Bond::face_value`].
///
/// A floating rate note cannot project its cash without fixings;
/// [`BondContract::entitled_cash`](crate::contracts::BondContract::entitled_cash) needs a
/// fixing only for a coupon that falls between the two dates.
///
/// # Errors
///
/// [`BondError::InvalidSpec`] when `to` is before `from`, the face is not positive or an
/// amount overflows; the bond's own error when it cannot project its cash.
pub fn entitled_cash<B: Bond + ?Sized>(bond: &B, from: Date, to: Date) -> BondResult<Decimal> {
    if to == from {
        return Ok(Decimal::ZERO);
    }
    if to < from {
        return Err(BondError::invalid_spec(
            "the later settlement date is before the earlier",
        ));
    }
    let face = bond.face_value();
    if face <= Decimal::ZERO {
        return Err(BondError::invalid_spec("the bond's face is not positive"));
    }
    let owed_from = bond.try_cash_flows(from)?;
    let owed_to = bond.try_cash_flows(to)?;
    let amounts = owed_from
        .iter()
        .map(|flow| flow.amount)
        .chain(owed_to.iter().map(|flow| -flow.amount));
    per_100_face(amounts, face)
        .map_err(|_| BondError::invalid_spec("entitled cash exceeds what a Decimal holds"))
}

/// `Σ amounts × 100 / face`, exact until the one rounding to a `Decimal`.
pub(crate) fn per_100_face(
    amounts: impl IntoIterator<Item = Decimal>,
    face: Decimal,
) -> Result<Decimal, MathError> {
    let mut cash = BoundedDecimal::zero();
    for amount in amounts {
        cash.add(&BoundedDecimal::decimal(amount))?;
    }
    cash.multiply(Decimal::ONE_HUNDRED)
        .and_then(|cash| cash.ratio(&BoundedDecimal::decimal(face)))
        .map(|cash| cash.normalize())
}
