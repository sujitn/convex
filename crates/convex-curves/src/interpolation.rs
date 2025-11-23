//! Interpolation methods for yield curves.

use serde::{Deserialize, Serialize};

/// Interpolation methods for yield curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum InterpolationMethod {
    /// Linear interpolation on zero rates.
    #[default]
    Linear,

    /// Linear interpolation on log discount factors.
    LogLinear,

    /// Cubic spline on zero rates.
    CubicSpline,

    /// Cubic spline on log discount factors.
    CubicSplineOnDiscount,

    /// Monotone convex interpolation (Hagan-West).
    MonotoneConvex,

    /// Nelson-Siegel parametric model.
    NelsonSiegel,

    /// Svensson parametric model.
    Svensson,

    /// Flat forward rates.
    FlatForward,
}

impl InterpolationMethod {
    /// Returns true if this is a parametric model.
    #[must_use]
    pub fn is_parametric(&self) -> bool {
        matches!(self, Self::NelsonSiegel | Self::Svensson)
    }

    /// Returns true if this method produces smooth curves.
    #[must_use]
    pub fn is_smooth(&self) -> bool {
        matches!(
            self,
            Self::CubicSpline
                | Self::CubicSplineOnDiscount
                | Self::MonotoneConvex
                | Self::NelsonSiegel
                | Self::Svensson
        )
    }
}

impl std::fmt::Display for InterpolationMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Linear => "Linear",
            Self::LogLinear => "Log-Linear",
            Self::CubicSpline => "Cubic Spline",
            Self::CubicSplineOnDiscount => "Cubic Spline (Discount)",
            Self::MonotoneConvex => "Monotone Convex",
            Self::NelsonSiegel => "Nelson-Siegel",
            Self::Svensson => "Svensson",
            Self::FlatForward => "Flat Forward",
        };
        write!(f, "{name}")
    }
}
