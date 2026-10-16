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

const SCALE: u32 = 184;
const MAX_BITS: u64 = 1536;

fn error() -> MathError {
    MathError::invalid_input("supplied risk arithmetic exceeds the admitted Decimal range")
}

fn power(exponent: u32) -> BigInt {
    BigInt::from(10u8).pow(exponent)
}

/// Bounded exact decimal state. Callers enforce their own row/factor work limits.
#[derive(Debug, Clone)]
pub struct BoundedDecimal(BigInt);

impl BoundedDecimal {
    /// Construct exact zero.
    pub fn zero() -> Self {
        Self(BigInt::from(0))
    }

    /// Lift an exact native Decimal without rounding.
    pub fn decimal(value: Decimal) -> Self {
        Self(BigInt::from(value.mantissa()) * power(SCALE - value.scale()))
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
        if &coefficient % &divisor != BigInt::from(0) {
            return Err(error());
        }
        Self::checked(coefficient / divisor)
    }

    /// Divide by 10 to an exponent at most eight, without losing any digit.
    pub fn divide_power_of_ten(&self, exponent: u32) -> MathResult<Self> {
        if exponent > 8 {
            return Err(error());
        }
        let divisor = power(exponent);
        if &self.0 % &divisor != BigInt::from(0) {
            return Err(error());
        }
        Self::checked(&self.0 / divisor)
    }

    /// Round only the final Decimal using nearest/even; reject range and nonzero erasure.
    pub fn finish(&self) -> MathResult<Decimal> {
        Self::finish_ratio(&self.0, &power(SCALE))
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
            let scaled = &magnitude * power(scale);
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
