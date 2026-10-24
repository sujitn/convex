//! Explicit supplied-date Treasury bill quotation (Appendix B VI, 2022 basis).
//!
//! All rates are annual decimals; cash is absolute original face. Auction price
//! rounding is separate from unrounded inversion and analytic quote sensitivity.
//! No curve, business-day adjustment, receipt, history or provider is inferred.

use convex_core::{contracts::ExactDecimal, Currency, Date};
use rust_decimal::{Decimal as D, RoundingStrategy};
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Intrinsic supplied bill economics. Adapter reference metadata stays external.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TreasuryBillTerms {
    /// Original issue date, including for a later settlement.
    pub issue_date: Date,
    /// Original contractual maturity, without business-day adjustment.
    pub maturity: Date,
    /// Explicit denomination; this convention admits USD only.
    pub currency: Currency,
    /// Positive original absolute face, not a per100 price.
    pub face_value: ExactDecimal,
    /// Actual redemption; this convention requires original face redemption.
    pub redemption_value: ExactDecimal,
}

/// Exactly one supplied quote, with distinct absolute cash and decimal rates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TreasuryBillQuote {
    /// Unrounded absolute purchase cash for the original face.
    Price {
        /// Positive actual absolute cash.
        price_original_face: ExactDecimal,
    },
    /// Bank discount quote, on original face and actual/360.
    BankDiscount {
        /// Annual decimal (0.05 means 5 percent).
        decimal_rate: ExactDecimal,
    },
    /// Treasury investment rate, with the issue-based year and half-year branch.
    Investment {
        /// Annual decimal (0.05 means 5 percent).
        decimal_rate: ExactDecimal,
    },
}

/// The actual admitted investment-rate equation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TreasuryBillMethod {
    /// At most one half of the issue-based year: P = F/(1+r*i/y).
    Simple,
    /// Longer: P = F/[(1+(r/y-.5)*i)*(1+i/2)].
    CouponEquivalent,
}

/// Analytic sensitivity to the named decimal quote, at fixed dates and face.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreasuryBillSensitivity {
    /// Signed derivative of unrounded price per100 per decimal rate.
    pub d_price_per100_d_decimal_rate: ExactDecimal,
    /// Second derivative of unrounded price per100 per decimal rate squared.
    pub d2_price_per100_d_decimal_rate2: ExactDecimal,
    /// Signed tangent price change per100 for a +1bp quote change.
    pub signed_price_per100_per_up_1bp: ExactDecimal,
}

/// Complete unrounded quotation, separate auction rounding and exact residual.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreasuryBillResult {
    /// Original intrinsic terms, retained without a synthetic price or convention.
    pub terms: TreasuryBillTerms,
    /// Exactly supplied quote, retained independently from derived quantities.
    pub quote: TreasuryBillQuote,
    /// Actual active quote settlement.
    pub settlement: Date,
    /// Original issue-based year (365 or366), unchanged by later settlement.
    pub basis_days: u32,
    /// Actual remaining calendar days.
    pub remaining_days: u32,
    /// Actual half-year branch.
    pub method: TreasuryBillMethod,
    /// Unrounded absolute cash for original face.
    pub price_original_face: ExactDecimal,
    /// Unrounded price per100 original face.
    pub price_per100: ExactDecimal,
    /// Per100 price rounded to six decimals, midpoint away from zero.
    pub auction_price_per100: ExactDecimal,
    /// Actual/360 bank discount rate, decimal units.
    pub bank_discount_decimal: ExactDecimal,
    /// Treasury investment rate, decimal units.
    pub investment_decimal: ExactDecimal,
    /// Repricing difference per100 using the derived investment rate.
    pub native_price_residual_per100: ExactDecimal,
    /// Sensitivities to the Treasury investment rate, not a curve-node shock.
    pub investment_risk: TreasuryBillSensitivity,
    /// Sensitivities to the bank discount rate, separate from investment risk.
    pub bank_discount_risk: TreasuryBillSensitivity,
}

/// Typed financial admission, bounded arithmetic and inversion failures.
#[derive(Debug, Error)]
pub enum TreasuryBillError {
    /// Supplied dates, cash or quote violates the admitted positive-price domain.
    #[error("invalid Treasury bill input: {0}")]
    InvalidInput(&'static str),
    /// A supplied economic convention requires a different, complete owner.
    #[error("unsupported Treasury bill convention: {0}")]
    Unsupported(&'static str),
    /// Exact Decimal arithmetic or the declared cash/ratio domain was exceeded.
    #[error("Treasury bill resource limit: bounded cash/ratio or Decimal arithmetic")]
    ResourceLimit,
    /// Bounded square-root refinement or final native repricing failed.
    #[error("Treasury bill inversion did not meet its native repricing bound")]
    NoConvergence,
}
type Result<T> = std::result::Result<T, TreasuryBillError>;
fn checked(value: Option<D>) -> Result<D> {
    value.ok_or(TreasuryBillError::ResourceLimit)
}
fn add(a: D, b: D) -> Result<D> {
    checked(a.checked_add(b))
}
fn sub(a: D, b: D) -> Result<D> {
    checked(a.checked_sub(b))
}
fn mul(a: D, b: D) -> Result<D> {
    checked(a.checked_mul(b))
}
fn div(a: D, b: D) -> Result<D> {
    checked(a.checked_div(b))
}
fn exact(value: D) -> ExactDecimal {
    ExactDecimal::new(value)
}
fn positive(value: D) -> Result<D> {
    if value <= D::ZERO {
        Err(TreasuryBillError::InvalidInput(
            "positive cash and investment discount factors required",
        ))
    } else {
        Ok(value)
    }
}

// Decimal's generic sqrt has a panic circuit breaker. This owner refines with
// checked arithmetic and a fixed work ceiling, then independently checks price.
fn sqrt(value: D) -> Result<D> {
    positive(value)?;
    let mut root = value.max(D::ONE);
    let tolerance = mul(value.max(D::ONE), dec!(0.000000000000000000000001))?;
    for _ in 0..96 {
        let next = div(add(root, div(value, root)?)?, dec!(2))?;
        if sub(next, root)?.abs() <= tolerance {
            return Ok(next);
        }
        root = next;
    }
    Err(TreasuryBillError::NoConvergence)
}

fn investment_price(face: D, rate: D, t: D, method: TreasuryBillMethod) -> Result<D> {
    let denominator = match method {
        TreasuryBillMethod::Simple => positive(add(D::ONE, mul(t, rate)?)?)?,
        TreasuryBillMethod::CouponEquivalent => {
            // Both factors must be positive: the other quadratic branch is
            // economically inadmissible even if their product is positive.
            let first = positive(add(D::ONE, mul(sub(t, dec!(0.5))?, rate)?)?)?;
            let second = positive(add(D::ONE, div(rate, dec!(2))?)?)?;
            mul(first, second)?
        }
    };
    div(face, denominator)
}

/// Quote complete original Treasury bill economics with bounded Decimal math.
///
/// Face:1e-15..1e15; price/face:1e-6..1e6. Issue/maturity must be within the
/// declared one-year interval, with issue<=settlement<maturity. A February29
/// issue lacks a same-date anniversary in the cited definition and is explicitly
/// unsupported. Risk differentiates unrounded price, with no quantity or curve
/// sensitivity inferred. Inversion always reprices within1e-18 price per100.
pub fn quote_treasury_bill(
    terms: &TreasuryBillTerms,
    settlement: Date,
    quote: &TreasuryBillQuote,
) -> Result<TreasuryBillResult> {
    if !(1..=9998).contains(&terms.issue_date.year())
        || !(1..=9999).contains(&terms.maturity.year())
        || settlement < terms.issue_date
        || settlement >= terms.maturity
        || terms.issue_date >= terms.maturity
    {
        return Err(TreasuryBillError::InvalidInput(
            "bounded active issue/settlement/maturity required",
        ));
    }
    let face = positive(terms.face_value.decimal())?;
    if terms.currency != Currency::USD || terms.face_value != terms.redemption_value {
        return Err(TreasuryBillError::Unsupported(
            "USD original-face redemption required",
        ));
    }
    if terms.issue_date.month() == 2 && terms.issue_date.day() == 29 {
        return Err(TreasuryBillError::Unsupported(
            "February29 issue requires an explicit anniversary convention",
        ));
    }
    let anniversary = terms
        .issue_date
        .add_years(1)
        .map_err(|_| TreasuryBillError::InvalidInput("issue anniversary out of range"))?;
    if terms.maturity > anniversary {
        return Err(TreasuryBillError::Unsupported(
            "Treasury bill maturity exceeds its one-year issue interval",
        ));
    }
    if !(dec!(0.000000000000001)..=dec!(1000000000000000)).contains(&face) {
        return Err(TreasuryBillError::ResourceLimit);
    }
    let basis_days = terms.issue_date.days_between(&anniversary) as u32;
    let remaining_days = settlement.days_between(&terms.maturity) as u32;
    let t = div(D::from(remaining_days), D::from(basis_days))?;
    let method = if remaining_days * 2 <= basis_days {
        TreasuryBillMethod::Simple
    } else {
        TreasuryBillMethod::CouponEquivalent
    };
    let a = if method == TreasuryBillMethod::Simple {
        D::ZERO
    } else {
        div(sub(t, dec!(0.5))?, dec!(2))?
    };
    let price = positive(match quote {
        TreasuryBillQuote::Price {
            price_original_face,
        } => price_original_face.decimal(),
        TreasuryBillQuote::BankDiscount { decimal_rate } => mul(
            face,
            sub(
                D::ONE,
                div(
                    mul(decimal_rate.decimal(), D::from(remaining_days))?,
                    dec!(360),
                )?,
            )?,
        )?,
        TreasuryBillQuote::Investment { decimal_rate } => {
            investment_price(face, decimal_rate.decimal(), t, method)?
        }
    })?;
    let ratio = div(price, face)?;
    if !(dec!(0.000001)..=dec!(1000000)).contains(&ratio) {
        return Err(TreasuryBillError::ResourceLimit);
    }
    let gain = sub(div(face, price)?, D::ONE)?;
    let rate = if method == TreasuryBillMethod::Simple {
        div(gain, t)?
    } else {
        // Rationalized admissible root: avoids subtraction cancellation near par
        // and near the half-year boundary; no approximate long-bill substitution.
        let discriminant = add(mul(t, t)?, mul(mul(dec!(4), a)?, gain)?)?;
        div(mul(dec!(2), gain)?, add(t, sqrt(discriminant)?)?)?
    };
    let p100 = mul(ratio, dec!(100))?;
    let reprice = investment_price(face, rate, t, method)?;
    let residual = mul(div(sub(reprice, price)?, face)?, dec!(100))?;
    if residual.abs() > dec!(0.000000000000000001) {
        return Err(TreasuryBillError::NoConvergence);
    }
    let den = div(face, reprice)?;
    let slope = add(t, mul(mul(dec!(2), a)?, rate)?)?;
    let den2 = mul(den, den)?;
    let first = div(mul(dec!(-100), slope)?, den2)?;
    let second = mul(
        dec!(100),
        sub(
            div(mul(dec!(2), mul(slope, slope)?)?, mul(den2, den)?)?,
            div(mul(dec!(2), a)?, den2)?,
        )?,
    )?;
    let sensitivity = |first: D, second: D| -> Result<TreasuryBillSensitivity> {
        Ok(TreasuryBillSensitivity {
            d_price_per100_d_decimal_rate: exact(first),
            d2_price_per100_d_decimal_rate2: exact(second),
            signed_price_per100_per_up_1bp: exact(div(first, dec!(10000))?),
        })
    };
    Ok(TreasuryBillResult {
        terms: terms.clone(),
        quote: quote.clone(),
        settlement,
        basis_days,
        remaining_days,
        method,
        price_original_face: exact(price),
        price_per100: exact(p100),
        auction_price_per100: exact(
            p100.round_dp_with_strategy(6, RoundingStrategy::MidpointAwayFromZero),
        ),
        bank_discount_decimal: exact(div(
            mul(sub(D::ONE, ratio)?, dec!(360))?,
            D::from(remaining_days),
        )?),
        investment_decimal: exact(rate),
        native_price_residual_per100: exact(residual),
        investment_risk: sensitivity(first, second)?,
        bank_discount_risk: sensitivity(
            -div(mul(dec!(100), D::from(remaining_days))?, dec!(360))?,
            D::ZERO,
        )?,
    })
}
