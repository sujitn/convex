//! Money market yield calculations.
//!
//! These yields are used for short-term instruments like T-Bills,
//! commercial paper, and other money market instruments.

use crate::YasError;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Calculate discount yield (bank discount basis).
///
/// Used for T-Bills and other discount instruments.
///
/// # Formula
///
/// ```text
/// Discount Yield = (Face - Price) / Face × (360 / Days)
/// ```
///
/// # Arguments
///
/// * `price` - Purchase price per $100 face
/// * `face_value` - Face value (usually 100)
/// * `days_to_maturity` - Days until maturity
///
/// # Returns
///
/// Discount yield as percentage
pub fn discount_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> Result<Decimal, YasError> {
    if days_to_maturity == 0 {
        return Err(YasError::InvalidInput(
            "days to maturity must be positive".to_string(),
        ));
    }

    if face_value <= Decimal::ZERO {
        return Err(YasError::InvalidInput(
            "face value must be positive".to_string(),
        ));
    }

    let discount = face_value - price;
    let days = Decimal::from(days_to_maturity);
    let dy = discount / face_value * (dec!(360) / days) * dec!(100);
    Ok(dy)
}

/// Calculate bond equivalent yield (BEY).
///
/// Converts discount yield to a yield that can be compared with
/// coupon-bearing bonds.
///
/// # Formula (for instruments ≤ 182 days)
///
/// ```text
/// BEY = (Face - Price) / Price × (365 / Days)
/// ```
///
/// # Arguments
///
/// * `price` - Purchase price per $100 face
/// * `face_value` - Face value (usually 100)
/// * `days_to_maturity` - Days until maturity
///
/// # Returns
///
/// Bond equivalent yield as percentage
pub fn bond_equivalent_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> Result<Decimal, YasError> {
    if days_to_maturity == 0 {
        return Err(YasError::InvalidInput(
            "days to maturity must be positive".to_string(),
        ));
    }

    if price <= Decimal::ZERO {
        return Err(YasError::InvalidInput("price must be positive".to_string()));
    }

    let discount = face_value - price;
    let days = Decimal::from(days_to_maturity);

    if days_to_maturity <= 182 {
        // Simple formula for short-dated instruments
        let bey = discount / price * (dec!(365) / days) * dec!(100);
        Ok(bey)
    } else {
        // For longer instruments, use more complex formula
        // BEY = [-2d/365 + 2√((d/365)² + (2d/365 - 1)(1 - 100/P))] / (2d/365 - 1)
        let d = days_to_maturity as f64;
        let p = price.to_string().parse::<f64>().unwrap_or(100.0);
        let f = face_value.to_string().parse::<f64>().unwrap_or(100.0);

        let term = d / 365.0;
        let price_factor = 1.0 - f / p;
        let discriminant = term * term + (2.0 * term - 1.0) * price_factor;

        if discriminant < 0.0 {
            return Err(YasError::CalculationFailed(
                "negative discriminant in BEY calculation".to_string(),
            ));
        }

        let bey = (-2.0 * term + 2.0 * discriminant.sqrt()) / (2.0 * term - 1.0);
        Ok(Decimal::from_f64_retain(bey * 100.0).unwrap_or(Decimal::ZERO))
    }
}

/// Calculate CD equivalent yield.
///
/// Used for comparing discount instruments with CDs that quote
/// on an add-on interest basis.
///
/// # Formula
///
/// ```text
/// CD Equivalent = (Face - Price) / Price × (360 / Days)
/// ```
pub fn cd_equivalent_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> Result<Decimal, YasError> {
    if days_to_maturity == 0 {
        return Err(YasError::InvalidInput(
            "days to maturity must be positive".to_string(),
        ));
    }

    if price <= Decimal::ZERO {
        return Err(YasError::InvalidInput("price must be positive".to_string()));
    }

    let discount = face_value - price;
    let days = Decimal::from(days_to_maturity);
    let cd = discount / price * (dec!(360) / days) * dec!(100);
    Ok(cd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_discount_yield() {
        // T-Bill: 98.5 price, 100 face, 90 days
        let price = dec!(98.5);
        let face = dec!(100.0);
        let days = 90;

        let dy = discount_yield(price, face, days).unwrap();

        // (100 - 98.5) / 100 × (360/90) × 100 = 6.0%
        assert_relative_eq!(dy.to_string().parse::<f64>().unwrap(), 6.0, epsilon = 0.01);
    }

    #[test]
    fn test_bond_equivalent_yield_short() {
        // 90-day T-Bill at 98.5
        let price = dec!(98.5);
        let face = dec!(100.0);
        let days = 90;

        let bey = bond_equivalent_yield(price, face, days).unwrap();

        // (100 - 98.5) / 98.5 × (365/90) × 100 ≈ 6.17%
        assert_relative_eq!(
            bey.to_string().parse::<f64>().unwrap(),
            6.17,
            epsilon = 0.02
        );
    }

    #[test]
    fn test_cd_equivalent_yield() {
        // 90-day instrument at 98.5
        let price = dec!(98.5);
        let face = dec!(100.0);
        let days = 90;

        let cd = cd_equivalent_yield(price, face, days).unwrap();

        // (100 - 98.5) / 98.5 × (360/90) × 100 ≈ 6.09%
        assert_relative_eq!(cd.to_string().parse::<f64>().unwrap(), 6.09, epsilon = 0.02);
    }
}
