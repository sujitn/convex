//! Bounded exact arithmetic for supplied Holding factor risk.
//!
//! Six Decimal factors plus price/basis-point divisions and one half need at most
//! 179 decimal places. A fixed scale of 184 retains every supplied digit.
//! At the one-million-row work bound, the coefficients stay below 1230 bits;
//! an explicit 1536-bit ceiling bounds all exact states. Existing key-rate
//! output rounding and all 105 reference books remain unchanged.
//! Only final reported Decimal outputs use nearest/even rounding.

use crate::{PortfolioError, PortfolioResult};
use num_bigint::{BigInt, Sign};
use rust_decimal::Decimal;

const SCALE: u32 = 184;
const MAX_BITS: u64 = 1536;

fn error() -> PortfolioError {
    PortfolioError::invalid_portfolio("supplied risk arithmetic exceeds the admitted Decimal range")
}

fn power(exponent: u32) -> BigInt {
    BigInt::from(10u8).pow(exponent)
}

/// Private bounded exact value; callers enforce holding/factor/work limits.
#[derive(Clone)]
pub(crate) struct ExactRisk(BigInt);

impl ExactRisk {
    pub(crate) fn zero() -> Self {
        Self(BigInt::from(0))
    }

    pub(crate) fn decimal(value: Decimal) -> Self {
        Self(BigInt::from(value.mantissa()) * power(SCALE - value.scale()))
    }

    fn checked(coefficient: BigInt) -> PortfolioResult<Self> {
        if coefficient.bits() > MAX_BITS {
            return Err(error());
        }
        Ok(Self(coefficient))
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.0.sign() == Sign::NoSign
    }

    pub(crate) fn is_negative(&self) -> bool {
        self.0.sign() == Sign::Minus
    }

    pub(crate) fn add(&mut self, value: &Self) -> PortfolioResult<()> {
        *self = Self::checked(&self.0 + &value.0)?;
        Ok(())
    }

    pub(crate) fn multiply(&self, value: Decimal) -> PortfolioResult<Self> {
        let coefficient = &self.0 * BigInt::from(value.mantissa());
        let divisor = power(value.scale());
        if &coefficient % &divisor != BigInt::from(0) {
            return Err(error());
        }
        Self::checked(coefficient / divisor)
    }

    pub(crate) fn divide_power_of_ten(&self, exponent: u32) -> PortfolioResult<Self> {
        if exponent > 8 {
            return Err(error());
        }
        let divisor = power(exponent);
        if &self.0 % &divisor != BigInt::from(0) {
            return Err(error());
        }
        Self::checked(&self.0 / divisor)
    }

    pub(crate) fn finish(&self) -> PortfolioResult<Decimal> {
        Self::finish_ratio(&self.0, &power(SCALE))
    }

    pub(crate) fn ratio(&self, denominator: &Self) -> PortfolioResult<Decimal> {
        Self::finish_ratio(&self.0, &denominator.0)
    }

    fn finish_ratio(numerator: &BigInt, denominator: &BigInt) -> PortfolioResult<Decimal> {
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
