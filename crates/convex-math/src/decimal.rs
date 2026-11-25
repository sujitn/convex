//! Bounded exact decimal products, signed accumulation and final ratios.
//!
//! Six Decimal factors plus price/basis-point divisions and one half need at most
//! 179 decimal places. A fixed scale of 184 retains every supplied digit.
//! At the one-million-row work bound, the coefficients stay below 1230 bits;
//! an explicit 1536-bit ceiling bounds all exact states. Existing key-rate
//! output rounding and all 105 reference books remain unchanged.
//! Only final reported Decimal outputs use nearest/even rounding.

use crate::{MathError, MathResult};
use num_bigint::{BigInt, Sign};
use rust_decimal::Decimal;
use std::borrow::Cow;
use std::sync::LazyLock;

const SCALE: u32 = 184;
const MAX_BITS: u64 = 1536;

fn error() -> MathError {
    MathError::invalid_input("supplied risk arithmetic exceeds the admitted Decimal range")
}

/// Ten to each exponent from 0 to `SCALE`, built once on first use.
static POWERS: LazyLock<Vec<BigInt>> = LazyLock::new(|| {
    let mut powers = Vec::with_capacity(SCALE as usize + 1);
    let mut value = BigInt::from(1u8);
    for _ in 0..=SCALE {
        powers.push(value.clone());
        value *= 10u8;
    }
    powers
});

/// Ten to `exponent`. Every exponent used here is at most `SCALE` and is read from the
/// table; a larger one would be computed, never clamped.
fn power(exponent: u32) -> Cow<'static, BigInt> {
    POWERS.get(exponent as usize).map_or_else(
        || Cow::Owned(BigInt::from(10u8).pow(exponent)),
        Cow::Borrowed,
    )
}

/// Bounded exact decimal state. Callers enforce their own row/factor work limits.
///
/// Every state holds its coefficient at the same fixed scale, so the derived order is
/// exact numeric order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BoundedDecimal(BigInt);

impl BoundedDecimal {
    /// Construct exact zero.
    pub fn zero() -> Self {
        Self(BigInt::from(0))
    }

    /// Lift an exact native Decimal without rounding.
    pub fn decimal(value: Decimal) -> Self {
        Self(BigInt::from(value.mantissa()) * &*power(SCALE - value.scale()))
    }

    fn checked(coefficient: BigInt) -> MathResult<Self> {
        if coefficient.bits() > MAX_BITS {
            return Err(error());
        }
        Ok(Self(coefficient))
    }

    /// Whether the exact state is zero.
    pub fn is_zero(&self) -> bool {
        self.0.sign() == Sign::NoSign
    }

    /// Whether the exact state is negative.
    pub fn is_negative(&self) -> bool {
        self.0.sign() == Sign::Minus
    }

    /// Accumulate another exact state, rejecting the fixed bit bound.
    pub fn add(&mut self, value: &Self) -> MathResult<()> {
        *self = Self::checked(&self.0 + &value.0)?;
        Ok(())
    }

    /// Multiply by an exact Decimal, rejecting lost digits or excess state.
    pub fn multiply(&self, value: Decimal) -> MathResult<Self> {
        let coefficient = &self.0 * BigInt::from(value.mantissa());
        let divisor = power(value.scale());
        if &coefficient % &*divisor != BigInt::from(0) {
            return Err(error());
        }
        Self::checked(coefficient / &*divisor)
    }

    /// Divide by 10 to an exponent at most eight, without losing any digit.
    pub fn divide_power_of_ten(&self, exponent: u32) -> MathResult<Self> {
        if exponent > 8 {
            return Err(error());
        }
        let divisor = power(exponent);
        if &self.0 % &*divisor != BigInt::from(0) {
            return Err(error());
        }
        Self::checked(&self.0 / &*divisor)
    }

    /// Round only the final Decimal using nearest/even; reject range and nonzero erasure.
    pub fn finish(&self) -> MathResult<Decimal> {
        Self::finish_ratio(&self.0, &power(SCALE))
    }

    /// Finish an exact scalar without choosing a rounding policy.
    ///
    /// Rejects overflow or any digits that a native Decimal cannot retain.
    /// Existing rounded currency/risk finishing remains in [`Self::finish`].
    pub fn finish_exact(&self) -> MathResult<Decimal> {
        let failure =
            || MathError::invalid_input("exact result exceeds Decimal range or precision");
        let divisor = power(SCALE - 28);
        if &self.0 % &*divisor != BigInt::from(0) {
            return Err(failure());
        }
        let mut coefficient = &self.0 / &*divisor;
        let mut scale = 28;
        while coefficient.bits() > 96 && scale > 0 {
            if &coefficient % 10u8 != BigInt::from(0) {
                return Err(failure());
            }
            coefficient /= 10u8;
            scale -= 1;
        }
        if coefficient.bits() > 96 {
            return Err(failure());
        }
        let coefficient = i128::try_from(coefficient).map_err(|_| failure())?;
        Decimal::try_from_i128_with_scale(coefficient, scale)
            .map(|value| value.normalize())
            .map_err(|_| failure())
    }

    /// `a + b` as [`Self::finish`] rounds it: nearest/even to what a `Decimal` holds, at
    /// the finest scale that holds it. A sum 128-bit integers hold is rounded in them,
    /// digit for digit as `finish` would, and any other through `finish` itself.
    ///
    /// # Errors
    ///
    /// As [`Self::finish`]: a sum beyond the `Decimal` range.
    pub fn rounded_sum(a: Decimal, b: Decimal) -> MathResult<Decimal> {
        if let Some(sum) = rounded_sum_in_i128(a, b) {
            return Ok(sum);
        }
        let mut sum = Self::decimal(a);
        sum.add(&Self::decimal(b))?;
        sum.finish()
    }

    /// Round the exact ratio at the final output; denominator must be positive.
    pub fn ratio(&self, denominator: &Self) -> MathResult<Decimal> {
        Self::finish_ratio(&self.0, &denominator.0)
    }

    /// Round the exact ratio directly to nearest/even IEEE binary64.
    /// The denominator must be positive. Nonfinite output and erased nonzero
    /// output fail. Fixed 1536-bit states require at most 53 extra temporary bits.
    pub fn ratio_as_f64(&self, denominator: &Self) -> MathResult<f64> {
        if denominator.0.sign() != Sign::Plus {
            return Err(error());
        }
        if self.is_zero() {
            return Ok(0.0);
        }
        let n = BigInt::from(self.0.magnitude().clone());
        let d = &denominator.0;
        let mut exponent = i32::try_from(n.bits()).map_err(|_| error())?
            - i32::try_from(d.bits()).map_err(|_| error())?;
        let below = if exponent >= 0 {
            n < (d << exponent as usize)
        } else {
            (&n << (-exponent) as usize) < *d
        };
        if below {
            exponent -= 1;
        }
        if exponent > 1023 {
            return Err(error());
        }
        let normal = exponent >= -1022;
        let shift = if normal { 52 - exponent } else { 1074 };
        let (scaled_n, scaled_d) = if shift >= 0 {
            (n << shift as usize, d.clone())
        } else {
            (n, d << (-shift) as usize)
        };
        let mut coefficient = &scaled_n / &scaled_d;
        let remainder = (&scaled_n % &scaled_d) * 2u8;
        if remainder > scaled_d
            || (remainder == scaled_d && &coefficient % 2u8 == BigInt::from(1u8))
        {
            coefficient += 1u8;
        }
        let mut mantissa = u64::try_from(coefficient).map_err(|_| error())?;
        if mantissa == 0 {
            return Err(error());
        }
        let sign = if self.is_negative() { 1u64 << 63 } else { 0 };
        let bits = if normal {
            if mantissa == (1u64 << 53) {
                mantissa >>= 1;
                exponent += 1;
            }
            if exponent > 1023 {
                return Err(error());
            }
            sign | (((exponent + 1023) as u64) << 52) | (mantissa - (1u64 << 52))
        } else {
            sign | mantissa
        };
        let value = f64::from_bits(bits);
        if !value.is_finite() || value == 0.0 {
            return Err(error());
        }
        Ok(value)
    }

    fn finish_ratio(numerator: &BigInt, denominator: &BigInt) -> MathResult<Decimal> {
        if denominator.sign() != Sign::Plus {
            return Err(error());
        }
        if numerator.sign() == Sign::NoSign {
            return Ok(Decimal::ZERO);
        }
        let magnitude = BigInt::from(numerator.magnitude().clone());
        let maximum = BigInt::from((1u128 << 96) - 1);
        if magnitude > &maximum * denominator {
            return Err(error());
        }
        for scale in (0..=28).rev() {
            let scaled = &magnitude * &*power(scale);
            let mut coefficient = &scaled / denominator;
            let twice_remainder = (&scaled % denominator) * 2u8;
            if twice_remainder > *denominator
                || (twice_remainder == *denominator && &coefficient % 2u8 == BigInt::from(1))
            {
                coefficient += 1u8;
            }
            if coefficient > maximum {
                continue;
            }
            if coefficient.sign() == Sign::NoSign {
                return Err(error());
            }
            let coefficient = i128::try_from(coefficient).map_err(|_| error())?;
            let signed = if numerator.sign() == Sign::Minus {
                -coefficient
            } else {
                coefficient
            };
            return Decimal::try_from_i128_with_scale(signed, scale).map_err(|_| error());
        }
        Err(error())
    }
}

/// [`BoundedDecimal::rounded_sum`] in 128-bit integers: the rounding of
/// [`BoundedDecimal::finish_ratio`] for a sum of two `Decimal`s, so it never builds a big
/// integer. `None` when the sum outgrows 128 bits at the finer scale of the two, or rounds
/// out of range, which `finish` then reports.
fn rounded_sum_in_i128(a: Decimal, b: Decimal) -> Option<Decimal> {
    const MAX_COEFFICIENT: u128 = (1 << 96) - 1;
    const MAX_DECIMAL_SCALE: u32 = 28;
    let scale = a.scale().max(b.scale());
    let lift = |value: Decimal| {
        value
            .mantissa()
            .checked_mul(10i128.checked_pow(scale - value.scale())?)
    };
    let sum = lift(a)?.checked_add(lift(b)?)?;
    if sum == 0 {
        return Some(Decimal::ZERO);
    }
    let magnitude = sum.unsigned_abs();
    // `finish_ratio` refuses a sum beyond the range before it rounds.
    if MAX_COEFFICIENT
        .checked_mul(10u128.pow(scale))
        .is_some_and(|most| magnitude > most)
    {
        return None;
    }
    // The finest scale whose rounded coefficient fits 96 bits, as `finish_ratio` finds it.
    for finest in (0..=MAX_DECIMAL_SCALE).rev() {
        let coefficient = if finest >= scale {
            match magnitude.checked_mul(10u128.pow(finest - scale)) {
                Some(coefficient) => coefficient,
                None => continue,
            }
        } else {
            let divisor = 10u128.pow(scale - finest);
            let (quotient, remainder) = (magnitude / divisor, magnitude % divisor);
            let twice = remainder * 2;
            if twice > divisor || (twice == divisor && quotient % 2 == 1) {
                quotient + 1
            } else {
                quotient
            }
        };
        if coefficient > MAX_COEFFICIENT {
            continue;
        }
        if coefficient == 0 {
            return None;
        }
        let coefficient = i128::try_from(coefficient).ok()?;
        let signed = if sum < 0 { -coefficient } else { coefficient };
        return Decimal::try_from_i128_with_scale(signed, finest).ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{power, rounded_sum_in_i128, BigInt, BoundedDecimal, Cow, SCALE};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    /// The sum in 128-bit integers is what `finish` gives, digit for digit and scale for
    /// scale, whenever it gives one.
    #[test]
    fn a_sum_rounded_in_128_bits_is_the_finished_sums_value_and_scale() {
        let finished = |a: Decimal, b: Decimal| {
            let mut sum = BoundedDecimal::decimal(a);
            sum.add(&BoundedDecimal::decimal(b)).unwrap();
            sum.finish()
        };
        let values = [
            dec!(0),
            dec!(0.25),
            dec!(0.5),
            dec!(1.25),
            dec!(99.25),
            dec!(100),
            dec!(101.123456789),
            dec!(0.0000000000000000000000000001),
            dec!(0.0000000000000000000000000005),
            dec!(0.5138888888888888888888888889),
            dec!(2.6875000000000000000000000001),
            dec!(7922816251426433759354395033.5),
            dec!(79228162514264337593543950335),
            dec!(-2.375),
            dec!(-0.0000000000000000000000000005),
        ];
        let mut rounded = 0;
        for a in values {
            for b in values {
                let expected = finished(a, b).ok().map(|sum| sum.to_string());
                // `None` leaves the sum to `finish`.
                if let Some(sum) = rounded_sum_in_i128(a, b) {
                    rounded += 1;
                    assert_eq!(Some(sum.to_string()), expected, "{a} + {b}");
                }
                assert_eq!(
                    BoundedDecimal::rounded_sum(a, b)
                        .ok()
                        .map(|sum| sum.to_string()),
                    expected,
                    "{a} + {b}"
                );
            }
        }
        assert!(rounded > 150, "{rounded}");
        // Past the range before rounding, as `finish` refuses it.
        assert_eq!(rounded_sum_in_i128(Decimal::MAX, dec!(0.25)), None);
        assert!(BoundedDecimal::rounded_sum(Decimal::MAX, dec!(0.25)).is_err());
        // A price and an accrued of 28 places, as a runtime's results hold them.
        for (clean, accrued) in [
            (dec!(99.25), dec!(0.5138888888888888888888888889)),
            (dec!(101.123456789), dec!(2.6875000000000000000000000001)),
            (dec!(100), dec!(0.0000000000000000000000000005)),
        ] {
            assert!(
                rounded_sum_in_i128(clean, accrued).is_some(),
                "{clean} + {accrued}"
            );
        }
    }

    #[test]
    fn every_power_of_ten_equals_its_computed_value() {
        for exponent in 0..=SCALE + 2 {
            let expected = BigInt::from(10u8).pow(exponent);
            assert_eq!(*power(exponent), expected, "10^{exponent}");
        }
    }

    #[test]
    fn the_table_holds_every_power_to_the_scale_and_no_further() {
        assert!(matches!(power(0), Cow::Borrowed(_)));
        assert!(matches!(power(SCALE), Cow::Borrowed(_)));
        assert!(matches!(power(SCALE + 1), Cow::Owned(_)));
    }
}
