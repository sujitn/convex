//! Compounding conventions for interest rate calculations.
//!
//! This module re-exports [`Compounding`] from `convex-core` and provides
//! curve-specific constants for convenience.
//!
//! # Note on Default
//!
//! The default compounding is `SemiAnnual` (the standard for US bonds).
//! For yield curve construction where `Continuous` compounding is preferred,
//! use `Compounding::Continuous` explicitly.
//!
//! # Example
//!
//! ```rust
//! use convex_curves::Compounding;
//!
//! let rate = 0.05; // 5% rate
//! let t = 2.0;     // 2 years
//!
//! let df_continuous = Compounding::Continuous.discount_factor(rate, t);
//! let df_annual = Compounding::Annual.discount_factor(rate, t);
//!
//! // Continuous compounding gives slightly lower DF
//! assert!(df_continuous < df_annual);
//! ```

// Re-export Compounding from convex-core as the canonical implementation
pub use convex_core::types::Compounding;

/// Default compounding for curve operations.
///
/// While `Compounding::default()` returns `SemiAnnual` (standard for bonds),
/// curve operations typically use `Continuous` compounding. Use this constant
/// for clarity when constructing curves.
pub const CURVE_COMPOUNDING: Compounding = Compounding::Continuous;

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_discount_factor_continuous() {
        let rate = 0.05;
        let t = 1.0;
        let df = Compounding::Continuous.discount_factor(rate, t);
        // DF = e^(-0.05) ≈ 0.9512
        assert_relative_eq!(df, (-0.05_f64).exp(), epsilon = 1e-10);
    }

    #[test]
    fn test_discount_factor_semi_annual() {
        let rate = 0.05;
        let t = 1.0;
        let df = Compounding::SemiAnnual.discount_factor(rate, t);
        // DF = (1 + 0.025)^(-2) ≈ 0.9518
        let expected = (1.0 + 0.025_f64).powf(-2.0);
        assert_relative_eq!(df, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_discount_factor_simple() {
        let rate = 0.05;
        let t = 0.5;
        let df = Compounding::Simple.discount_factor(rate, t);
        // DF = 1 / (1 + 0.05 * 0.5) = 1/1.025 ≈ 0.9756
        assert_relative_eq!(df, 1.0 / 1.025, epsilon = 1e-10);
    }

    #[test]
    fn test_zero_rate_continuous() {
        let df = 0.9512294245;
        let t = 1.0;
        let rate = Compounding::Continuous.zero_rate(df, t);
        assert_relative_eq!(rate, 0.05, epsilon = 1e-4);
    }

    #[test]
    fn test_roundtrip_all_compounding() {
        let original_rate = 0.05;
        let t = 2.0;

        for compounding in [
            Compounding::Continuous,
            Compounding::Annual,
            Compounding::SemiAnnual,
            Compounding::Quarterly,
            Compounding::Monthly,
            Compounding::Simple,
        ] {
            let df = compounding.discount_factor(original_rate, t);
            let recovered_rate = compounding.zero_rate(df, t);
            assert_relative_eq!(
                recovered_rate,
                original_rate,
                epsilon = 1e-10,
                max_relative = 1e-10
            );
        }
    }

    #[test]
    fn test_convert_semi_annual_to_continuous() {
        let semi_rate = 0.05;
        let t = 1.0;

        let cont_rate = Compounding::SemiAnnual.convert_to(semi_rate, Compounding::Continuous, t);

        // Verify by computing DF both ways
        let df_semi = Compounding::SemiAnnual.discount_factor(semi_rate, t);
        let df_cont = Compounding::Continuous.discount_factor(cont_rate, t);

        assert_relative_eq!(df_semi, df_cont, epsilon = 1e-10);
    }

    #[test]
    fn test_convert_continuous_to_annual() {
        let cont_rate = 0.05;
        let t = 1.0;

        let annual_rate = Compounding::Continuous.convert_to(cont_rate, Compounding::Annual, t);

        // Continuous 5% ≈ Annual 5.127% (since e^0.05 - 1 ≈ 0.05127)
        let expected = (0.05_f64).exp() - 1.0;
        assert_relative_eq!(annual_rate, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_periods_per_year_opt() {
        assert_eq!(Compounding::Continuous.periods_per_year_opt(), None);
        assert_eq!(Compounding::Simple.periods_per_year_opt(), None);
        assert_eq!(Compounding::Annual.periods_per_year_opt(), Some(1));
        assert_eq!(Compounding::SemiAnnual.periods_per_year_opt(), Some(2));
        assert_eq!(Compounding::Quarterly.periods_per_year_opt(), Some(4));
        assert_eq!(Compounding::Monthly.periods_per_year_opt(), Some(12));
    }

    #[test]
    fn test_curve_compounding_constant() {
        assert_eq!(CURVE_COMPOUNDING, Compounding::Continuous);
    }

    #[test]
    fn test_zero_time_returns_one() {
        for compounding in [
            Compounding::Continuous,
            Compounding::Annual,
            Compounding::SemiAnnual,
            Compounding::Simple,
        ] {
            assert_eq!(compounding.discount_factor(0.05, 0.0), 1.0);
        }
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Compounding::Continuous), "Continuous");
        assert_eq!(format!("{}", Compounding::SemiAnnual), "Semi-Annual");
    }
}
