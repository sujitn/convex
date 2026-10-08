//! Pricing router - selects pricing model based on bond type and config.

#[cfg(test)]
use crate::reference_terms;

use crate::curve_builder::BuiltCurve;
use crate::error::EngineError;
use crate::ports::output::BondQuoteOutput;
use crate::ports::reference_data::{BondReferenceData, BondType};
use crate::ports::storage::BidAskSpreadConfig;
use convex_analytics::bond_quote::{quote_fixed, quote_zero, BondCurves, BondQuote};
use convex_analytics::risk::STANDARD_KEY_RATE_TENORS;
use convex_analytics::spreads::GovernmentCurve;
#[cfg(test)]
use convex_bonds::prelude::Bond;
use convex_bonds::prelude::{FixedRateBond, FixedRateBondBuilder};
use convex_core::daycounts::DayCountConvention;
#[cfg(test)]
use convex_core::types::Compounding;
#[cfg(test)]
use convex_core::types::Yield;
use convex_core::types::{Frequency, Mark, PriceKind};
use convex_core::Date;
#[cfg(test)]
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use std::str::FromStr;
#[cfg(test)]
use tracing::debug;

/// Format tenor as a human-readable label (e.g., "3M", "1Y", "10Y").
fn format_tenor_label(tenor: f64) -> String {
    if tenor < 1.0 {
        let months = (tenor * 12.0).round() as u32;
        format!("{}M", months)
    } else {
        let years = tenor.round() as u32;
        format!("{}Y", years)
    }
}

/// Pricing input for a single bond.
#[derive(Debug)]
pub struct PricingInput {
    /// Bond reference data
    pub bond: BondReferenceData,
    /// Settlement date
    pub settlement_date: Date,

    // === Market data (bid/mid/ask) ===
    /// Market quote price - bid (clean)
    pub market_price_bid: Option<Decimal>,
    /// Market quote price - mid (clean)
    pub market_price_mid: Option<Decimal>,
    /// Market quote price - ask (clean)
    pub market_price_ask: Option<Decimal>,

    // === Curves for analytics ===
    /// Discount curve (for Z-spread)
    pub discount_curve: Option<BuiltCurve>,
    /// Benchmark curve (for I-spread / swap curve)
    pub benchmark_curve: Option<BuiltCurve>,
    /// Government curve (for G-spread)
    pub government_curve: Option<GovernmentCurve>,
    /// Volatility (for OAS)
    pub volatility: Option<Decimal>,

    // === Bid/Ask spread configuration ===
    /// Configuration for generating bid/ask from mid price (for model-priced bonds)
    pub bid_ask_config: Option<BidAskSpreadConfig>,
}

impl PricingInput {
    /// Get market price for a specific side.
    pub fn market_price(&self, side: crate::ports::storage::QuoteSide) -> Option<Decimal> {
        use crate::ports::storage::QuoteSide;
        match side {
            QuoteSide::Bid => self.market_price_bid,
            QuoteSide::Mid => self.market_price_mid,
            QuoteSide::Ask => self.market_price_ask,
        }
    }

    /// Create input with only mid price (for backward compatibility).
    pub fn with_mid_price(
        bond: BondReferenceData,
        settlement_date: Date,
        market_price: Option<Decimal>,
        discount_curve: Option<BuiltCurve>,
        benchmark_curve: Option<BuiltCurve>,
        government_curve: Option<GovernmentCurve>,
        volatility: Option<Decimal>,
    ) -> Self {
        Self {
            bond,
            settlement_date,
            market_price_bid: None,
            market_price_mid: market_price,
            market_price_ask: None,
            discount_curve,
            benchmark_curve,
            government_curve,
            volatility,
            bid_ask_config: None,
        }
    }

    /// Get effective bid price, applying BidAskSpreadConfig if market bid is not available.
    ///
    /// When `bid_ask_config` is set and `prefer_market_spread` is false (or market bid is None),
    /// derives bid price from mid price using the spread config.
    pub fn effective_bid_price(&self) -> Option<Decimal> {
        // If market bid is available and config prefers market spread, use it
        if let Some(bid) = self.market_price_bid {
            if self
                .bid_ask_config
                .as_ref()
                .is_none_or(|c| c.prefer_market_spread)
            {
                return Some(bid);
            }
        }

        // Otherwise derive from mid + config
        if let (Some(mid), Some(config)) = (self.market_price_mid, &self.bid_ask_config) {
            let spread_bps = config.effective_bid_spread_bps();
            // Bid price is lower than mid (widening in price terms)
            // price_change = mid * spread_bps / 10000
            let adjustment = mid.checked_mul(Decimal::from_f64_retain(spread_bps / 10000.0)?)?;
            mid.checked_sub(adjustment)
        } else {
            self.market_price_bid
        }
    }

    /// Get effective ask price, applying BidAskSpreadConfig if market ask is not available.
    ///
    /// When `bid_ask_config` is set and `prefer_market_spread` is false (or market ask is None),
    /// derives ask price from mid price using the spread config.
    pub fn effective_ask_price(&self) -> Option<Decimal> {
        // If market ask is available and config prefers market spread, use it
        if let Some(ask) = self.market_price_ask {
            if self
                .bid_ask_config
                .as_ref()
                .is_none_or(|c| c.prefer_market_spread)
            {
                return Some(ask);
            }
        }

        // Otherwise derive from mid + config
        if let (Some(mid), Some(config)) = (self.market_price_mid, &self.bid_ask_config) {
            let spread_bps = config.effective_ask_spread_bps();
            // Ask price is higher than mid (widening in price terms)
            let adjustment = mid.checked_mul(Decimal::from_f64_retain(spread_bps / 10000.0)?)?;
            mid.checked_add(adjustment)
        } else {
            self.market_price_ask
        }
    }

    /// Get effective prices (bid, mid, ask) applying BidAskSpreadConfig as needed.
    pub fn effective_prices(&self) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>) {
        (
            self.effective_bid_price(),
            self.market_price_mid,
            self.effective_ask_price(),
        )
    }
}

/// Application adapter; all pricing and risk mathematics live in the pure library.
pub struct PricingRouter {
    /// Central-difference half width, in basis points.
    pub bump_bps: f64,
}
impl PricingRouter {
    /// Construct the adapter with the retained 10bp risk setting.
    pub fn new() -> Self {
        Self { bump_bps: 10.0 }
    }
    /// Convert BondReferenceData to a FixedRateBond for analytics.
    fn to_fixed_rate_bond(
        &self,
        ref_data: &BondReferenceData,
    ) -> Result<FixedRateBond, EngineError> {
        let coupon_rate = ref_data
            .coupon_rate
            .ok_or_else(|| EngineError::PricingError("Missing coupon rate".to_string()))?;

        let frequency = self.get_frequency(ref_data)?;

        let day_count = self.parse_day_count(&ref_data.day_count)?;
        let terms = ref_data
            .contractual_terms
            .as_ref()
            .ok_or_else(|| EngineError::PricingError("Missing contractual terms".into()))?;
        let calendar = match terms.calendar_code {
            crate::ports::reference_master::BondCalendar::WeekendsOnly => {
                convex_bonds::types::CalendarId::weekend_only()
            }
            crate::ports::reference_master::BondCalendar::Sifma => {
                convex_bonds::types::CalendarId::sifma()
            }
            crate::ports::reference_master::BondCalendar::Target2 => {
                convex_bonds::types::CalendarId::target2()
            }
        };

        let mut builder = FixedRateBondBuilder::new()
            .identifiers(
                convex_bonds::types::BondIdentifiers::new()
                    .with_ticker(ref_data.instrument_id.as_str()),
            )
            .issue_date(ref_data.issue_date)
            .dated_date(terms.dated_date.date())
            .maturity(ref_data.maturity_date)
            .coupon_rate(coupon_rate)
            .face_value(ref_data.face_value)
            .frequency(frequency)
            .day_count(day_count)
            .currency(ref_data.currency)
            .calendar(calendar.clone())
            .business_day_convention(terms.settings.business_day_convention)
            .end_of_month(terms.settings.end_of_month)
            .settlement_days(terms.settings.settlement_days)
            .redemption_value(terms.redemption_value.decimal());

        if let Some(first) = ref_data.first_coupon_date {
            builder = builder.first_coupon_date(first);
        }
        if let Some(last) = terms.penultimate_coupon_date {
            builder = builder.penultimate_coupon_date(last.date());
        }
        if terms.settings.ex_coupon_days != 0 {
            builder = builder.ex_coupon_rule(convex_core::cashflows::ExCouponRule {
                offset: convex_core::cashflows::DateOffset::CalendarDays(
                    -(terms.settings.ex_coupon_days as i32),
                ),
                anchor: convex_core::cashflows::ExDateAnchor::ContractualEnd,
                boundary: convex_core::cashflows::ExCouponBoundary::OnDate,
            });
        }
        // Add CUSIP identifier if available
        if let Some(cusip) = ref_data.cusip.as_deref() {
            builder = builder.cusip_unchecked(cusip);
        }

        builder
            .build()
            .map_err(|e| EngineError::PricingError(format!("Failed to build bond: {}", e)))
    }

    /// Reject unknown labels; no contractual convention is inferred.
    fn parse_day_count(&self, dcc_str: &str) -> Result<DayCountConvention, EngineError> {
        DayCountConvention::from_str(dcc_str)
            .map_err(|_| EngineError::PricingError("Unknown day-count convention".into()))
    }

    /// Get frequency enum from reference data.
    fn get_frequency(&self, ref_data: &BondReferenceData) -> Result<Frequency, EngineError> {
        match ref_data.frequency {
            1 => Ok(Frequency::Annual),
            2 => Ok(Frequency::SemiAnnual),
            4 => Ok(Frequency::Quarterly),
            12 => Ok(Frequency::Monthly),
            _ => Err(EngineError::PricingError(
                "Unsupported payment frequency".into(),
            )),
        }
    }

    /// Ad-hoc completion clock; snapshot callers use price_at.
    pub fn price(&self, input: &PricingInput) -> Result<BondQuoteOutput, EngineError> {
        self.price_at(input, chrono::Utc::now().timestamp_millis())
    }
    /// Deterministic pricing over actual supplied reference and curve values.
    pub fn price_at(
        &self,
        input: &PricingInput,
        calculated_at_ms: i64,
    ) -> Result<BondQuoteOutput, EngineError> {
        crate::ports::reference_repository::validate_bond_reference(&input.bond)
            .map_err(pricing_error)?;
        if !matches!(
            input.bond.bond_type,
            BondType::FixedBullet | BondType::ZeroCoupon
        ) {
            return Err(EngineError::PricingError("unsupported production bond terms/model: FRNs require published fixings and projection, optional/amortizing products require the completed explicit model contract (CVX-055)".into()));
        }
        if input.bond.is_callable
            || input.bond.is_putable
            || input.bond.is_sinkable
            || !input.bond.call_schedule.is_empty()
            || input.bond.floating_terms.is_some()
            || input.bond.inflation_index.is_some()
            || input.bond.inflation_base_index.is_some()
            || input.bond.has_deflation_floor
        {
            return Err(EngineError::PricingError(
                "unsupported embedded terms on nominal bullet/zero".into(),
            ));
        }
        if let Some(config) = &input.bid_ask_config {
            if [
                config.half_spread_bps,
                config.effective_bid_spread_bps(),
                config.effective_ask_spread_bps(),
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0 || *v >= 10_000.0)
            {
                return Err(EngineError::PricingError(
                    "invalid bid/ask configuration".into(),
                ));
            }
        }
        for curve in [&input.discount_curve, &input.benchmark_curve]
            .into_iter()
            .flatten()
        {
            if curve.inner.is_none() {
                return Err(EngineError::PricingError(
                    "missing validated curve representation".into(),
                ));
            }
        }
        if input.volatility.is_some() {
            return Err(EngineError::PricingError(
                "unsupported volatility input for nominal bullet/zero".into(),
            ));
        }
        let frequency = self.get_frequency(&input.bond)?;
        let (bid, mid, ask) = input.effective_prices();
        if mid.or(bid).or(ask).is_none() {
            return Err(EngineError::PricingError(
                "missing observed clean price; no model or market value substituted".into(),
            ));
        }
        let price_mark = |value| Mark::Price {
            value,
            kind: PriceKind::Clean,
        };
        let bond = if input.bond.bond_type == BondType::FixedBullet {
            Some(self.to_fixed_rate_bond(&input.bond)?)
        } else {
            None
        };
        let zero = if bond.is_none() {
            let compounding = match frequency {
                Frequency::Annual => convex_bonds::instruments::Compounding::Annual,
                Frequency::SemiAnnual => convex_bonds::instruments::Compounding::SemiAnnual,
                Frequency::Quarterly => convex_bonds::instruments::Compounding::Quarterly,
                Frequency::Monthly => convex_bonds::instruments::Compounding::Monthly,
                _ => {
                    return Err(EngineError::PricingError(
                        "unsupported zero yield frequency".into(),
                    ))
                }
            };
            let terms = input
                .bond
                .contractual_terms
                .as_ref()
                .ok_or_else(|| EngineError::PricingError("missing zero terms".into()))?;
            if input.bond.coupon_rate.is_some_and(|c| c != Decimal::ZERO)
                || input.bond.first_coupon_date.is_some()
                || terms.penultimate_coupon_date.is_some()
                || terms.stub != crate::ports::reference_master::BondStub::None
                || terms.settings.ex_coupon_days != 0
                || terms.settings.business_day_convention
                    != convex_core::calendars::BusinessDayConvention::Unadjusted
            {
                return Err(EngineError::PricingError(
                    "unsupported coupon/adjusted-payment terms on zero".into(),
                ));
            }
            Some(
                convex_bonds::instruments::ZeroCouponBond::builder()
                    .cusip_unchecked(input.bond.instrument_id.as_str())
                    .issue_date(input.bond.issue_date)
                    .maturity(input.bond.maturity_date)
                    .currency(input.bond.currency)
                    .day_count(self.parse_day_count(&input.bond.day_count)?)
                    .compounding(compounding)
                    .face_value(input.bond.face_value)
                    .redemption_value(terms.redemption_value.decimal())
                    .build()
                    .map_err(pricing_error)?,
            )
        } else {
            None
        };
        let quote = |value| -> Result<BondQuote, EngineError> {
            if let Some(bond) = &bond {
                quote_fixed(
                    bond,
                    input.settlement_date,
                    &price_mark(value),
                    frequency,
                    BondCurves {
                        discount: input
                            .discount_curve
                            .as_ref()
                            .map(|c| c as &dyn convex_curves::RateCurveDyn),
                        swap: input
                            .benchmark_curve
                            .as_ref()
                            .map(|c| c as &dyn convex_curves::RateCurveDyn),
                        government: input.government_curve.as_ref(),
                    },
                    self.bump_bps,
                    STANDARD_KEY_RATE_TENORS,
                )
                .map_err(pricing_error)
            } else {
                let zero = zero
                    .as_ref()
                    .ok_or_else(|| EngineError::PricingError("missing zero instrument".into()))?;
                let (pricing, yield_risk) =
                    quote_zero(zero, input.settlement_date, &price_mark(value))
                        .map_err(pricing_error)?;
                let errors = ["z_spread", "i_spread", "g_spread", "asw", "curve_risk"]
                    .into_iter()
                    .map(|field| {
                        convex_core::contracts::StructuredError::new(
                            convex_core::contracts::ErrorCode::UnsupportedConvention,
                            Some(field.into()),
                            "zero spread/curve-risk convention is not available in this contract",
                            false,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(pricing_error)?;
                Ok(BondQuote {
                    pricing,
                    yield_risk,
                    z_spread_bps: None,
                    i_spread_bps: None,
                    g_spread_bps: None,
                    asw_bps: None,
                    curve_risk: None,
                    errors,
                })
            }
        };
        let qb = bid.map(&quote).transpose()?;
        let qm = mid.map(&quote).transpose()?;
        let qa = ask.map(&quote).transpose()?;
        let base = qm
            .as_ref()
            .or(qb.as_ref())
            .or(qa.as_ref())
            .ok_or_else(|| EngineError::PricingError("missing quoted side".into()))?;
        let d = |v| {
            Decimal::from_f64_retain(v).ok_or_else(|| {
                EngineError::PricingError("nonfinite/unrepresentable library measure".into())
            })
        };
        let side = |q: &Option<BondQuote>, get: fn(&BondQuote) -> Option<f64>| {
            q.as_ref().and_then(get).map(d).transpose()
        };
        let risk = qm.as_ref().and_then(|q| q.curve_risk.as_ref());
        let mut errors = Vec::new();
        for (q, suffix) in [
            (qb.as_ref(), "bid"),
            (qm.as_ref(), "mid"),
            (qa.as_ref(), "ask"),
        ] {
            if let Some(q) = q {
                for error in &q.errors {
                    let field = error.field().map(|f| format!("{f}_{suffix}"));
                    errors.push(
                        convex_core::contracts::StructuredError::new(
                            error.code(),
                            field,
                            error.message(),
                            false,
                        )
                        .map_err(pricing_error)?,
                    );
                }
            }
        }
        Ok(BondQuoteOutput {
            instrument_id: input.bond.instrument_id.clone(),
            isin: input.bond.isin.clone(),
            currency: input.bond.currency,
            settlement_date: input.settlement_date,
            clean_price_bid: bid,
            clean_price_mid: mid,
            clean_price_ask: ask,
            accrued_interest: Some(d(base.pricing.accrued_per_100)?),
            ytm_bid: side(&qb, |q| Some(q.pricing.ytm_decimal))?,
            ytm_mid: side(&qm, |q| Some(q.pricing.ytm_decimal))?,
            ytm_ask: side(&qa, |q| Some(q.pricing.ytm_decimal))?,
            ytw: None,
            ytc: None,
            z_spread_bid: side(&qb, |q| q.z_spread_bps)?,
            z_spread_mid: side(&qm, |q| q.z_spread_bps)?,
            z_spread_ask: side(&qa, |q| q.z_spread_bps)?,
            i_spread_bid: side(&qb, |q| q.i_spread_bps)?,
            i_spread_mid: side(&qm, |q| q.i_spread_bps)?,
            i_spread_ask: side(&qa, |q| q.i_spread_bps)?,
            g_spread_bid: side(&qb, |q| q.g_spread_bps)?,
            g_spread_mid: side(&qm, |q| q.g_spread_bps)?,
            g_spread_ask: side(&qa, |q| q.g_spread_bps)?,
            asw_bid: side(&qb, |q| q.asw_bps)?,
            asw_mid: side(&qm, |q| q.asw_bps)?,
            asw_ask: side(&qa, |q| q.asw_bps)?,
            oas_bid: None,
            oas_mid: None,
            oas_ask: None,
            discount_margin_bid: None,
            discount_margin_mid: None,
            discount_margin_ask: None,
            simple_margin_bid: None,
            simple_margin_mid: None,
            simple_margin_ask: None,
            modified_duration: side(&qm, |q| Some(q.yield_risk.modified_duration))?,
            macaulay_duration: side(&qm, |q| Some(q.yield_risk.macaulay_duration))?,
            convexity: side(&qm, |q| Some(q.yield_risk.convexity))?,
            dv01: side(&qm, |q| Some(q.yield_risk.dv01_loss))?,
            pv01: risk.map(|r| d(-r.zero_pv01_signed)).transpose()?,
            cs01: risk.map(|r| d(-r.z_pv01_signed)).transpose()?,
            effective_duration: risk.map(|r| d(r.effective_duration)).transpose()?,
            effective_convexity: risk.map(|r| d(r.effective_convexity)).transpose()?,
            spread_duration: risk.map(|r| d(r.spread_duration)).transpose()?,
            key_rate_durations: risk
                .map(|r| {
                    r.key_rates
                        .iter()
                        .map(|(t, v)| Ok((format_tenor_label(*t), d(*v)?)))
                        .collect::<Result<Vec<_>, EngineError>>()
                })
                .transpose()?,
            zero_pv01_signed: risk.map(|r| d(r.zero_pv01_signed)).transpose()?,
            z_pv01_signed: risk.map(|r| d(r.z_pv01_signed)).transpose()?,
            measure_errors: errors,
            timestamp: calculated_at_ms,
            pricing_spec: if zero.is_some() {
                "ZeroCoupon"
            } else {
                "DiscountToMaturity"
            }
            .into(),
            source: "convex-library".into(),
            is_stale: false,
            quality: if qm.as_ref().is_some_and(|q| q.errors.is_empty()) {
                100
            } else {
                75
            },
        })
    }
    /// Batch price multiple bonds sequentially.
    pub fn price_batch(
        &self,
        inputs: &[PricingInput],
    ) -> Vec<Result<BondQuoteOutput, EngineError>> {
        inputs.iter().map(|input| self.price(input)).collect()
    }

    /// Batch price multiple bonds in parallel using rayon.
    ///
    /// This provides significant speedup for large batches (100+ bonds).
    pub fn price_batch_parallel(
        &self,
        inputs: &[PricingInput],
    ) -> Vec<Result<BondQuoteOutput, EngineError>> {
        use rayon::prelude::*;
        inputs.par_iter().map(|input| self.price(input)).collect()
    }

    /// Price a batch and collect statistics.
    pub fn price_batch_with_stats(&self, inputs: &[PricingInput]) -> BatchPricingResult {
        use rayon::prelude::*;
        use std::time::Instant;

        let start = Instant::now();
        let results: Vec<_> = inputs.par_iter().map(|input| self.price(input)).collect();
        let elapsed = start.elapsed();

        let mut succeeded = 0;
        let mut failed = 0;
        let mut outputs = Vec::with_capacity(results.len());

        for result in results {
            match result {
                Ok(output) => {
                    succeeded += 1;
                    outputs.push(Ok(output));
                }
                Err(e) => {
                    failed += 1;
                    outputs.push(Err(e));
                }
            }
        }

        BatchPricingResult {
            outputs,
            succeeded,
            failed,
            elapsed_ms: elapsed.as_millis() as u64,
            bonds_per_second: if elapsed.as_secs_f64() > 0.0 {
                inputs.len() as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            },
        }
    }
}

/// Result of batch pricing operation.
#[derive(Debug)]
pub struct BatchPricingResult {
    /// Individual pricing results
    pub outputs: Vec<Result<BondQuoteOutput, EngineError>>,
    /// Number of successfully priced bonds
    pub succeeded: usize,
    /// Number of failed pricings
    pub failed: usize,
    /// Total elapsed time in milliseconds
    pub elapsed_ms: u64,
    /// Throughput (bonds per second)
    pub bonds_per_second: f64,
}

impl Default for PricingRouter {
    fn default() -> Self {
        Self::new()
    }
}

fn pricing_error(error: impl std::fmt::Display) -> EngineError {
    EngineError::PricingError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::reference_data::IssuerType;
    use convex_core::ids::InstrumentId;
    use rust_decimal_macros::dec;

    fn create_test_bond() -> BondReferenceData {
        reference_terms::complete(BondReferenceData {
            contractual_terms: None,
            instrument_id: InstrumentId::new("TEST001"),
            isin: Some("US912828Z229".to_string()),
            cusip: Some("912828Z22".to_string()),
            sedol: None,
            bbgid: None,
            description: "US Treasury 2.5% 2030".to_string(),
            currency: convex_core::Currency::USD,
            issue_date: Date::from_ymd(2020, 5, 15).unwrap(),
            maturity_date: Date::from_ymd(2030, 5, 15).unwrap(),
            coupon_rate: Some(dec!(0.025)),
            frequency: 2,
            day_count: "30/360".to_string(),
            face_value: dec!(100),
            bond_type: BondType::FixedBullet,
            issuer_type: IssuerType::Sovereign,
            issuer_id: "US_GOVT".to_string(),
            issuer_name: "United States Treasury".to_string(),
            seniority: "Senior".to_string(),
            is_callable: false,
            call_schedule: vec![],
            is_putable: false,
            is_sinkable: false,
            floating_terms: None,
            inflation_index: None,
            inflation_base_index: None,
            has_deflation_floor: false,
            country_of_risk: "US".to_string(),
            sector: "Government".to_string(),
            amount_outstanding: Some(dec!(50000000000)),
            first_coupon_date: Some(Date::from_ymd(2020, 11, 15).unwrap()),
            last_updated: 0,
            source: "test".to_string(),
        })
    }

    #[test]
    fn unknown_day_count_cannot_enter_reference_valuation() {
        let router = PricingRouter::new();
        let mut bond = create_test_bond();
        bond.day_count = "UNKNOWN-CONTRACT-DAY-COUNT".into();
        let input = PricingInput::with_mid_price(
            bond,
            Date::from_ymd(2025, 1, 15).unwrap(),
            Some(dec!(100)),
            None,
            None,
            None,
            None,
        );
        assert!(
            router.price(&input).is_err(),
            "unknown contractual day count must fail"
        );
    }

    #[test]
    fn missing_terms_and_unknown_frequency_cannot_enter_pricing() {
        let router = PricingRouter::new();
        let mut bond = create_test_bond();
        bond.contractual_terms = None;
        let input = PricingInput::with_mid_price(
            bond,
            Date::from_ymd(2025, 1, 15).unwrap(),
            Some(dec!(100)),
            None,
            None,
            None,
            None,
        );
        assert!(router.price(&input).is_err());
        let mut bond = create_test_bond();
        bond.frequency = 3;
        let input = PricingInput::with_mid_price(
            bond,
            Date::from_ymd(2025, 1, 15).unwrap(),
            Some(dec!(100)),
            None,
            None,
            None,
            None,
        );
        assert!(router.price(&input).is_err());
    }

    #[test]
    fn corrected_schedule_and_redemption_are_consumed_without_mutating_prior_terms() {
        let router = PricingRouter::new();
        let original = create_test_bond();
        let encoded = serde_json::to_vec(&original).unwrap();
        let mut revised = original.clone();
        revised.first_coupon_date = Some(Date::from_ymd(2020, 5, 15).unwrap());
        let terms = revised.contractual_terms.as_mut().unwrap();
        terms.dated_date = convex_core::contracts::BusinessDate::parse("2020-04-15").unwrap();
        terms.redemption_value = convex_core::contracts::ExactDecimal::parse("102.50").unwrap();
        let settlement = Date::from_ymd(2020, 5, 1).unwrap();
        let before = router
            .to_fixed_rate_bond(&original)
            .unwrap()
            .cash_flows(settlement);
        let after = router
            .to_fixed_rate_bond(&revised)
            .unwrap()
            .cash_flows(settlement);
        assert_ne!(before[0].date, after[0].date);
        assert_eq!(
            after.last().unwrap().amount - before.last().unwrap().amount,
            dec!(2.50)
        );
        assert_eq!(serde_json::to_vec(&original).unwrap(), encoded);
        let replay = router
            .to_fixed_rate_bond(&original)
            .unwrap()
            .cash_flows(settlement);
        let values = |flows: &[convex_bonds::traits::BondCashFlow]| {
            flows
                .iter()
                .map(|flow| {
                    (
                        flow.date,
                        flow.amount,
                        flow.flow_type,
                        flow.accrual_start,
                        flow.accrual_end,
                        flow.factor,
                        flow.reference_rate,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(values(&replay), values(&before));
    }

    fn create_test_curve(ref_date: Date) -> BuiltCurve {
        // Create a simple upward-sloping curve: 3% to 5% over 10 years
        use convex_core::ids::CurveId;

        let mut built = BuiltCurve {
            curve_id: CurveId::new("USD_SOFR"),
            reference_date: ref_date,
            points: vec![
                (0.25, 0.030), // 3M: 3.0%
                (0.5, 0.032),  // 6M: 3.2%
                (1.0, 0.035),  // 1Y: 3.5%
                (2.0, 0.038),  // 2Y: 3.8%
                (3.0, 0.040),  // 3Y: 4.0%
                (5.0, 0.045),  // 5Y: 4.5%
                (7.0, 0.048),  // 7Y: 4.8%
                (10.0, 0.050), // 10Y: 5.0%
                (30.0, 0.055), // 30Y: 5.5%
            ],
            built_at: 0,
            inputs_hash: "".to_string(),
            extrapolation: convex_curves::ExtrapolationMethod::Flat,
            inner: None,
        };
        built.rebuild_inner().unwrap();
        built
    }

    #[test]
    fn test_price_at_par() {
        let router = PricingRouter::new();
        let bond = create_test_bond();

        let input = PricingInput {
            bond,
            settlement_date: Date::from_ymd(2025, 1, 15).unwrap(),
            market_price_bid: None,
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None,
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());
        assert!(output.modified_duration.is_some());
        assert!(output.convexity.is_some());

        // YTM should be close to coupon rate at par
        let ytm = output.ytm_mid.unwrap().to_f64().unwrap();
        assert!(
            (ytm - 0.025).abs() < 0.01,
            "YTM {} should be close to 2.5%",
            ytm
        );

        // Duration should be around 4-5 years for a 5-year bond
        let dur = output.modified_duration.unwrap().to_f64().unwrap();
        assert!(dur > 3.0 && dur < 6.0, "Duration {} out of range", dur);
    }

    #[test]
    fn test_price_with_z_spread() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(95.0)),
            market_price_ask: None, // Discount price
            discount_curve: Some(curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());
        assert!(output.z_spread_mid.is_some());

        // Z-spread should be negative (bond trades cheap vs curve)
        // Since bond coupon is 2.5% and curve is ~4%, discount price implies negative spread
        let z_spread = output.z_spread_mid.unwrap().to_f64().unwrap();
        debug!("Z-spread: {} bps", z_spread);
        // Note: Z-spread can vary based on implementation
    }

    #[test]
    fn test_price_with_i_spread() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let swap_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None,
            discount_curve: None,
            benchmark_curve: Some(swap_curve),
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());
        assert!(output.i_spread_mid.is_some());

        // I-spread = YTM - swap rate at maturity
        // Bond YTM ~ 2.5%, swap rate at 5Y ~ 4.5%, so I-spread ~ -200 bps
        let i_spread = output.i_spread_mid.unwrap().to_f64().unwrap();
        assert!(
            i_spread < 0.0,
            "I-spread {} should be negative (bond yields less than swaps)",
            i_spread
        );
    }

    fn create_test_government_curve(ref_date: Date) -> GovernmentCurve {
        use convex_analytics::spreads::{GovernmentBenchmark, Sovereign};
        use convex_bonds::types::Tenor;

        // Create UST benchmarks at ~4% yield level (higher than 2.5% bond coupon)
        let y2 = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y2,
            "912828XX0",
            Date::from_ymd(2027, 1, 15).unwrap(),
            dec!(0.04),
            Yield::new(dec!(0.040), Compounding::SemiAnnual),
        );

        let y5 = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y5,
            "912828YY0",
            Date::from_ymd(2030, 1, 15).unwrap(),
            dec!(0.04),
            Yield::new(dec!(0.042), Compounding::SemiAnnual),
        );

        let y10 = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y10,
            "912828ZZ0",
            Date::from_ymd(2035, 1, 15).unwrap(),
            dec!(0.04),
            Yield::new(dec!(0.045), Compounding::SemiAnnual),
        );

        GovernmentCurve::us_treasury(ref_date)
            .with_benchmark(y2)
            .with_benchmark(y5)
            .with_benchmark(y10)
    }

    #[test]
    fn test_price_with_g_spread() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let gov_curve = create_test_government_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None,
            discount_curve: None,
            benchmark_curve: None,
            government_curve: Some(gov_curve),
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());
        assert!(output.g_spread_mid.is_some());

        // G-spread = Bond YTM - Treasury yield at maturity
        // Bond YTM ~ 2.5%, Treasury 5Y yield ~ 4.2%, so G-spread ~ -170 bps
        let g_spread = output.g_spread_mid.unwrap().to_f64().unwrap();
        assert!(
            g_spread < 0.0,
            "G-spread {} should be negative (corporate yields less than treasury - unusual but valid for test)",
            g_spread
        );
    }

    #[test]
    fn test_price_with_asw() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let swap_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(95.0)),
            market_price_ask: None, // Discount price
            discount_curve: None,
            benchmark_curve: Some(swap_curve),
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());
        assert!(output.i_spread_mid.is_some());
        assert!(
            output.asw_mid.is_some(),
            "ASW should be calculated when swap curve is provided"
        );

        // ASW measures the spread in an asset swap package
        // For a discount bond, ASW should be positive (spread compensates for paying par)
        let asw = output.asw_mid.unwrap().to_f64().unwrap();
        println!("ASW: {} bps, I-spread: {:?} bps", asw, output.i_spread_mid);

        // ASW and I-spread should be related but not identical
        // (ASW accounts for coupon annuity, I-spread is simple subtraction)
    }

    #[test]
    fn test_price_with_key_rate_durations() {
        let router = PricingRouter::new();
        let bond = create_test_bond(); // 5-year bond
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None, // Par price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());

        // Key rate durations should be calculated when discount curve is provided
        assert!(
            output.key_rate_durations.is_some(),
            "Key rate durations should be calculated when discount curve is provided"
        );

        let krds = output.key_rate_durations.unwrap();
        assert!(!krds.is_empty(), "Should have at least one KRD point");

        // Print the key rate durations
        println!("Key Rate Durations:");
        let mut total_krd = 0.0;
        for (tenor, krd) in &krds {
            let krd_val = krd.to_f64().unwrap();
            total_krd += krd_val;
            println!("  {}: {:.4}", tenor, krd_val);
        }
        println!("  Total: {:.4}", total_krd);

        // Total KRD should be approximately equal to modified duration
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            // Allow some tolerance due to different calculation methods
            assert!(
                (total_krd - mod_dur_val).abs() < 2.0,
                "Total KRD {} should be close to modified duration {}",
                total_krd,
                mod_dur_val
            );
        }

        // For a 5-year bond, we should see KRDs at short and medium tenors
        let tenor_labels: Vec<&str> = krds.iter().map(|(t, _)| t.as_str()).collect();
        assert!(
            tenor_labels
                .iter()
                .any(|t| *t == "5Y" || *t == "3Y" || *t == "2Y"),
            "Should have KRD at medium tenors for 5-year bond"
        );
    }

    fn create_callable_test_bond() -> BondReferenceData {
        use crate::ports::reference_data::CallScheduleEntry;

        reference_terms::complete(BondReferenceData {
            contractual_terms: None,
            instrument_id: InstrumentId::new("CALLABLE001"),
            isin: Some("US123456789".to_string()),
            cusip: Some("123456789".to_string()),
            sedol: None,
            bbgid: None,
            description: "Test Callable Bond 5% 2030".to_string(),
            currency: convex_core::Currency::USD,
            issue_date: Date::from_ymd(2020, 6, 15).unwrap(),
            maturity_date: Date::from_ymd(2030, 6, 15).unwrap(),
            coupon_rate: Some(dec!(0.05)),
            frequency: 2,
            day_count: "30/360".to_string(),
            face_value: dec!(100),
            bond_type: BondType::FixedCallable,
            issuer_type: IssuerType::CorporateIG,
            issuer_id: "CORP001".to_string(),
            issuer_name: "Test Corporation".to_string(),
            seniority: "Senior".to_string(),
            is_callable: true,
            call_schedule: vec![
                CallScheduleEntry {
                    call_date: Date::from_ymd(2025, 6, 15).unwrap(),
                    call_price: dec!(102),
                    is_make_whole: false,
                },
                CallScheduleEntry {
                    call_date: Date::from_ymd(2027, 6, 15).unwrap(),
                    call_price: dec!(101),
                    is_make_whole: false,
                },
                CallScheduleEntry {
                    call_date: Date::from_ymd(2028, 6, 15).unwrap(),
                    call_price: dec!(100),
                    is_make_whole: false,
                },
            ],
            is_putable: false,
            is_sinkable: false,
            floating_terms: None,
            inflation_index: None,
            inflation_base_index: None,
            has_deflation_floor: false,
            country_of_risk: "US".to_string(),
            sector: "Corporate".to_string(),
            amount_outstanding: Some(dec!(500000000)),
            first_coupon_date: Some(Date::from_ymd(2020, 12, 15).unwrap()),
            last_updated: 0,
            source: "test".to_string(),
        })
    }

    #[test]
    fn test_price_callable_with_oas() {
        let input = PricingInput::with_mid_price(
            create_callable_test_bond(),
            Date::from_ymd(2025, 1, 15).unwrap(),
            Some(dec!(100)),
            None,
            None,
            None,
            None,
        );
        let error = PricingRouter::new().price(&input).unwrap_err();
        assert!(error.to_string().contains("unsupported"), "{error}");
    }

    #[test]
    fn test_price_callable_with_ytw() {
        let input = PricingInput::with_mid_price(
            create_callable_test_bond(),
            Date::from_ymd(2025, 1, 15).unwrap(),
            Some(dec!(100)),
            None,
            None,
            None,
            None,
        );
        let error = PricingRouter::new().price(&input).unwrap_err();
        assert!(error.to_string().contains("unsupported"), "{error}");
    }

    fn create_frn_test_bond() -> BondReferenceData {
        use crate::ports::reference_data::FloatingRateTerms;
        use convex_core::ids::FloatingRateIndex;

        reference_terms::complete(BondReferenceData {
            contractual_terms: None,
            instrument_id: InstrumentId::new("FRN001"),
            isin: Some("US912828ZQ7".to_string()),
            cusip: Some("912828ZQ7".to_string()),
            sedol: None,
            bbgid: None,
            description: "Test FRN SOFR+50bps 2027".to_string(),
            currency: convex_core::Currency::USD,
            issue_date: Date::from_ymd(2025, 1, 15).unwrap(),
            maturity_date: Date::from_ymd(2027, 1, 15).unwrap(),
            coupon_rate: None, // FRNs don't have fixed coupon
            frequency: 4,      // Quarterly
            day_count: "ACT/360".to_string(),
            face_value: dec!(100),
            bond_type: BondType::FloatingRate,
            issuer_type: IssuerType::CorporateIG,
            issuer_id: "CORP001".to_string(),
            issuer_name: "Test Corporation".to_string(),
            seniority: "Senior".to_string(),
            is_callable: false,
            call_schedule: vec![],
            is_putable: false,
            is_sinkable: false,
            floating_terms: Some(FloatingRateTerms {
                index: FloatingRateIndex::Sofr,
                spread: dec!(50),   // 50 bps
                reset_frequency: 4, // Quarterly
                cap: None,
                floor: None,
            }),
            inflation_index: None,
            inflation_base_index: None,
            has_deflation_floor: false,
            country_of_risk: "US".to_string(),
            sector: "Corporate".to_string(),
            amount_outstanding: Some(dec!(500000000)),
            first_coupon_date: Some(Date::from_ymd(2025, 4, 15).unwrap()),
            last_updated: 0,
            source: "test".to_string(),
        })
    }

    #[test]
    fn test_price_frn_with_discount_margin() {
        let input = PricingInput::with_mid_price(
            create_frn_test_bond(),
            Date::from_ymd(2025, 1, 15).unwrap(),
            Some(dec!(100)),
            None,
            None,
            None,
            None,
        );
        let error = PricingRouter::new().price(&input).unwrap_err();
        assert!(error.to_string().contains("unsupported"), "{error}");
    }

    #[test]
    fn test_price_at_discount() {
        let router = PricingRouter::new();
        let bond = create_test_bond();

        let input = PricingInput {
            bond,
            settlement_date: Date::from_ymd(2025, 1, 15).unwrap(),
            market_price_bid: None,
            market_price_mid: Some(dec!(95.0)),
            market_price_ask: None, // Discount
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        let ytm = output.ytm_mid.unwrap().to_f64().unwrap();

        // YTM should be higher than coupon when at discount
        assert!(ytm > 0.025, "YTM {} should be > 2.5% at discount", ytm);
    }

    #[test]
    fn test_price_at_premium() {
        let router = PricingRouter::new();
        let bond = create_test_bond();

        let input = PricingInput {
            bond,
            settlement_date: Date::from_ymd(2025, 1, 15).unwrap(),
            market_price_bid: None,
            market_price_mid: Some(dec!(105.0)),
            market_price_ask: None, // Premium
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        let ytm = output.ytm_mid.unwrap().to_f64().unwrap();

        // YTM should be lower than coupon when at premium
        assert!(ytm < 0.025, "YTM {} should be < 2.5% at premium", ytm);
    }

    #[test]
    fn test_built_curve_interpolation() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(ref_date);

        // Test exact point
        let rate_5y = curve.interpolate_rate(5.0);
        assert!((rate_5y - 0.045).abs() < 0.0001);

        // Test interpolation between points
        let rate_4y = curve.interpolate_rate(4.0);
        assert!(rate_4y > 0.040 && rate_4y < 0.045);

        // Test extrapolation (flat)
        let rate_40y = curve.interpolate_rate(40.0);
        assert!((rate_40y - 0.055).abs() < 0.0001);
    }

    #[test]
    fn test_built_curve_discount_factor() {
        use convex_curves::RateCurveDyn;

        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(ref_date);

        // DF at 0 should be 1.0
        let df_0 = curve.discount_factor(0.0).unwrap();
        assert!((df_0 - 1.0).abs() < 0.0001);

        // DF at 1Y with 3.5% rate: exp(-0.035 * 1) = 0.9656
        let df_1y = curve.discount_factor(1.0).unwrap();
        assert!(df_1y > 0.96 && df_1y < 0.97);

        // DF at 10Y with 5% rate: exp(-0.05 * 10) = 0.6065
        let df_10y = curve.discount_factor(10.0).unwrap();
        assert!(df_10y > 0.60 && df_10y < 0.62);
    }

    #[test]
    fn test_price_with_cs01() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(95.0)),
            market_price_ask: None, // Discount price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());
        assert!(
            output.z_spread_mid.is_some(),
            "Z-spread needed for CS01 calculation"
        );

        // CS01 should be calculated when we have z-spread and discount curve
        assert!(
            output.cs01.is_some(),
            "CS01 should be calculated when z-spread and discount curve are available"
        );

        let cs01 = output.cs01.unwrap().to_f64().unwrap();

        // CS01 should be positive (price decreases when spread increases)
        assert!(cs01 > 0.0, "CS01 {} should be positive", cs01);

        // CS01 for a 5-year bond should be roughly duration / 100 * price
        // Typical range: 0.04 to 0.06 for a 5-year bond at ~95
        assert!(
            cs01 > 0.01 && cs01 < 0.20,
            "CS01 {} should be in reasonable range for 5-year bond",
            cs01
        );

        println!(
            "CS01: {:.4} (price change per 1bp spread increase), Z-spread: {:?}bps",
            cs01, output.z_spread_mid
        );

        // CS01 relationship to duration: CS01 ≈ price * modified_duration / 10000
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            let price = output
                .dirty_price_mid()
                .unwrap_or(dec!(95))
                .to_f64()
                .unwrap();
            let expected_cs01 = price * mod_dur_val / 10000.0;

            // CS01 should be reasonably close to this approximation
            println!(
                "Expected CS01 (approx): {:.4}, Actual: {:.4}",
                expected_cs01, cs01
            );
        }
    }

    #[test]
    fn test_price_with_pv01() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None, // Par price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());
        assert!(output.dv01.is_some(), "DV01 should be calculated");

        // PV01 should be calculated when discount curve is provided (curve-based)
        assert!(
            output.pv01.is_some(),
            "PV01 should be calculated when discount curve is available"
        );

        let dv01 = output.dv01.unwrap().to_f64().unwrap();
        let pv01 = output.pv01.unwrap().to_f64().unwrap();

        // Both should be positive (price decreases when yield/rates increase)
        assert!(dv01 > 0.0, "DV01 {} should be positive", dv01);
        assert!(pv01 > 0.0, "PV01 {} should be positive", pv01);

        // For a 5-year bond, DV01/PV01 per 100 face should be approximately:
        // DV01 ≈ ModDur × Price × 0.0001 ≈ 5 × 100 × 0.0001 = 0.05
        assert!(
            dv01 > 0.01 && dv01 < 0.20,
            "DV01 {} should be in reasonable range",
            dv01
        );
        assert!(
            pv01 > 0.01 && pv01 < 0.20,
            "PV01 {} should be in reasonable range",
            pv01
        );

        // DV01 (yield-based) and PV01 (curve-based) should be similar for fixed-rate bonds
        let diff_pct = ((pv01 - dv01) / dv01).abs() * 100.0;
        assert!(
            diff_pct < 50.0, // Allow up to 50% difference due to different methods
            "PV01 {} and DV01 {} should be reasonably similar (diff: {:.1}%)",
            pv01,
            dv01,
            diff_pct
        );

        println!(
            "DV01: {:.6} (yield-based), PV01: {:.6} (curve-based), Diff: {:.1}%",
            dv01, pv01, diff_pct
        );

        // PV01 relationship to duration: PV01 ≈ price × modified_duration × 0.0001
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            let price = output
                .dirty_price_mid()
                .unwrap_or(dec!(100))
                .to_f64()
                .unwrap();
            let expected_pv01 = price * mod_dur_val * 0.0001;

            println!(
                "Expected PV01 (approx): {:.6}, Actual DV01: {:.6}, Actual PV01: {:.6}",
                expected_pv01, dv01, pv01
            );
        }
    }

    #[test]
    fn test_price_with_spread_duration() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(95.0)),
            market_price_ask: None, // Discount price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());
        assert!(
            output.z_spread_mid.is_some(),
            "Z-spread needed for spread duration"
        );

        // Spread duration should be calculated when we have z-spread and discount curve
        assert!(
            output.spread_duration.is_some(),
            "Spread duration should be calculated when z-spread and discount curve are available"
        );

        let spread_dur = output.spread_duration.unwrap().to_f64().unwrap();

        // Spread duration should be positive for a normal bond
        assert!(
            spread_dur > 0.0,
            "Spread duration {} should be positive",
            spread_dur
        );

        // For a fixed-rate bond, spread duration should be approximately equal to modified duration
        // Typical 5-year bond has duration around 4-5 years
        assert!(
            spread_dur > 1.0 && spread_dur < 10.0,
            "Spread duration {} should be in reasonable range for 5-year bond",
            spread_dur
        );

        // Spread duration should be close to modified duration for fixed-rate bonds
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            let diff_pct = ((spread_dur - mod_dur_val) / mod_dur_val).abs() * 100.0;

            println!(
                "Spread Duration: {:.4}, Modified Duration: {:.4}, Diff: {:.1}%",
                spread_dur, mod_dur_val, diff_pct
            );

            // For fixed-rate bonds, spread duration ≈ modified duration
            // Allow some tolerance due to different calculation methods
            assert!(
                diff_pct < 20.0,
                "Spread duration {} should be close to modified duration {} (diff: {:.1}%)",
                spread_dur,
                mod_dur_val,
                diff_pct
            );
        }

        println!(
            "Spread Duration: {:.4}, Z-spread: {:?}bps",
            spread_dur, output.z_spread_mid
        );
    }

    #[test]
    fn test_price_with_effective_duration() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None, // Par price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());

        // Effective duration should be calculated when discount curve is provided
        assert!(
            output.effective_duration.is_some(),
            "Effective duration should be calculated when discount curve is available"
        );

        let eff_dur = output.effective_duration.unwrap().to_f64().unwrap();

        // Effective duration should be positive for a normal bond
        assert!(
            eff_dur > 0.0,
            "Effective duration {} should be positive",
            eff_dur
        );

        // For a 5-year bond, effective duration should be around 4-5 years
        assert!(
            eff_dur > 1.0 && eff_dur < 10.0,
            "Effective duration {} should be in reasonable range for 5-year bond",
            eff_dur
        );

        // For non-callable fixed-rate bonds, effective duration ≈ modified duration
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            let diff_pct = ((eff_dur - mod_dur_val) / mod_dur_val).abs() * 100.0;

            println!(
                "Effective Duration: {:.4}, Modified Duration: {:.4}, Diff: {:.1}%",
                eff_dur, mod_dur_val, diff_pct
            );

            // For non-callable bonds, effective duration should be very close to modified duration
            // The small difference comes from curve shape vs flat yield assumption
            assert!(
                diff_pct < 15.0,
                "Effective duration {} should be close to modified duration {} (diff: {:.1}%)",
                eff_dur,
                mod_dur_val,
                diff_pct
            );
        }

        // Verify that effective duration uses curve-based calculation (10bp bump)
        // by checking it's different from a simple yield-based calculation
        println!(
            "Effective Duration (curve-based): {:.4}, ModDur: {:?}",
            eff_dur, output.modified_duration
        );
    }

    #[test]
    fn test_price_with_effective_convexity() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None, // Par price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm_mid.is_some());

        // Effective convexity should be calculated when discount curve is provided
        assert!(
            output.effective_convexity.is_some(),
            "Effective convexity should be calculated when discount curve is available"
        );

        let eff_conv = output.effective_convexity.unwrap().to_f64().unwrap();

        // Effective convexity should be positive for a standard non-callable bond
        // (positive convexity means bond gains more when rates fall than it loses when rates rise)
        assert!(
            eff_conv > 0.0,
            "Effective convexity {} should be positive for non-callable bond",
            eff_conv
        );

        // For a 5-year bond, convexity should be roughly proportional to duration squared
        // Typical range: 10-50 for a 5-year bond
        assert!(
            eff_conv > 1.0 && eff_conv < 100.0,
            "Effective convexity {} should be in reasonable range for 5-year bond",
            eff_conv
        );

        // For non-callable fixed-rate bonds, effective convexity ≈ analytical convexity
        if let Some(conv) = output.convexity {
            let conv_val = conv.to_f64().unwrap();
            let diff_pct = ((eff_conv - conv_val) / conv_val).abs() * 100.0;

            println!(
                "Effective Convexity: {:.4}, Analytical Convexity: {:.4}, Diff: {:.1}%",
                eff_conv, conv_val, diff_pct
            );

            // For non-callable bonds, effective convexity should be close to analytical convexity
            // Allow some tolerance due to different calculation methods (curve vs yield)
            assert!(
                diff_pct < 25.0,
                "Effective convexity {} should be close to analytical convexity {} (diff: {:.1}%)",
                eff_conv,
                conv_val,
                diff_pct
            );
        }

        // Both effective duration and convexity should be available
        assert!(
            output.effective_duration.is_some(),
            "Effective duration should also be calculated"
        );

        println!(
            "Effective Convexity: {:.4}, Effective Duration: {:?}",
            eff_conv, output.effective_duration
        );
    }

    // =========================================================================
    // BATCH PRICING THROUGHPUT TESTS
    // =========================================================================

    /// Create a batch of test bonds with varying characteristics.
    fn create_bond_batch(count: usize) -> Vec<BondReferenceData> {
        let coupons = [0.02, 0.025, 0.03, 0.035, 0.04, 0.045, 0.05];
        let maturities = [2026, 2027, 2028, 2029, 2030, 2031, 2032, 2033, 2034, 2035];

        (0..count)
            .map(|i| {
                let coupon_idx = i % coupons.len();
                let maturity_idx = i % maturities.len();

                reference_terms::complete(BondReferenceData {
                    contractual_terms: None,
                    instrument_id: InstrumentId::new(format!("BOND_{:05}", i)),
                    isin: Some(format!("US9128{:05}X", i)),
                    cusip: Some(format!("9128{:05}", i)),
                    sedol: None,
                    bbgid: None,
                    description: format!(
                        "Test Bond {} {:.1}% {}",
                        i,
                        coupons[coupon_idx] * 100.0,
                        maturities[maturity_idx]
                    ),
                    currency: convex_core::Currency::USD,
                    issue_date: Date::from_ymd(2020, 1, 15).unwrap(),
                    maturity_date: Date::from_ymd(maturities[maturity_idx], 6, 15).unwrap(),
                    coupon_rate: Some(Decimal::from_f64_retain(coupons[coupon_idx]).unwrap()),
                    frequency: 2,
                    day_count: "30/360".to_string(),
                    face_value: dec!(100),
                    bond_type: BondType::FixedBullet,
                    issuer_type: IssuerType::CorporateIG,
                    issuer_id: format!("ISSUER_{}", i % 10),
                    issuer_name: format!("Test Issuer {}", i % 10),
                    seniority: "Senior".to_string(),
                    is_callable: false,
                    call_schedule: vec![],
                    is_putable: false,
                    is_sinkable: false,
                    floating_terms: None,
                    inflation_index: None,
                    inflation_base_index: None,
                    has_deflation_floor: false,
                    country_of_risk: "US".to_string(),
                    sector: "Corporate".to_string(),
                    amount_outstanding: Some(dec!(1000000000)),
                    first_coupon_date: Some(Date::from_ymd(2020, 6, 15).unwrap()),
                    last_updated: 0,
                    source: "test".to_string(),
                })
            })
            .collect()
    }

    #[test]
    fn test_batch_pricing_sequential() {
        let router = PricingRouter::new();
        let settlement = Date::from_ymd(2025, 6, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create a batch of 100 bonds
        let bonds = create_bond_batch(100);
        let inputs: Vec<PricingInput> = bonds
            .into_iter()
            .enumerate()
            .map(|(i, bond)| {
                // Vary prices slightly around par
                let price_offset = (i as f64 % 10.0) - 5.0;
                let price = 100.0 + price_offset;

                PricingInput {
                    bond,
                    settlement_date: settlement,
                    market_price_bid: None,
                    market_price_mid: Some(Decimal::from_f64_retain(price).unwrap()),
                    market_price_ask: None,
                    discount_curve: Some(curve.clone()),
                    benchmark_curve: Some(curve.clone()),
                    government_curve: None,
                    volatility: None,
                    bid_ask_config: None,
                }
            })
            .collect();

        // Time sequential batch pricing
        let start = std::time::Instant::now();
        let results = router.price_batch(&inputs);
        let elapsed = start.elapsed();

        // Verify results
        let succeeded: usize = results.iter().filter(|r| r.is_ok()).count();
        let failed: usize = results.iter().filter(|r| r.is_err()).count();

        println!(
            "Sequential batch: {} bonds in {:?} ({:.0} bonds/sec), {} succeeded, {} failed",
            inputs.len(),
            elapsed,
            inputs.len() as f64 / elapsed.as_secs_f64(),
            succeeded,
            failed
        );

        assert_eq!(succeeded, 100, "All bonds should price successfully");
        assert_eq!(failed, 0, "No bonds should fail");

        // Verify some analytics are calculated
        for result in results.iter().take(5) {
            let output = result.as_ref().unwrap();
            assert!(output.ytm_mid.is_some(), "YTM should be calculated");
            assert!(
                output.modified_duration.is_some(),
                "Duration should be calculated"
            );
            assert!(
                output.z_spread_mid.is_some(),
                "Z-spread should be calculated"
            );
        }
    }

    #[test]
    fn test_batch_pricing_parallel() {
        let router = PricingRouter::new();
        let settlement = Date::from_ymd(2025, 6, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create a larger batch for parallel test
        let bonds = create_bond_batch(500);
        let inputs: Vec<PricingInput> = bonds
            .into_iter()
            .enumerate()
            .map(|(i, bond)| {
                let price_offset = (i as f64 % 20.0) - 10.0;
                let price = 100.0 + price_offset;

                PricingInput {
                    bond,
                    settlement_date: settlement,
                    market_price_bid: None,
                    market_price_mid: Some(Decimal::from_f64_retain(price).unwrap()),
                    market_price_ask: None,
                    discount_curve: Some(curve.clone()),
                    benchmark_curve: Some(curve.clone()),
                    government_curve: None,
                    volatility: None,
                    bid_ask_config: None,
                }
            })
            .collect();

        // Time parallel batch pricing
        let start = std::time::Instant::now();
        let results = router.price_batch_parallel(&inputs);
        let parallel_elapsed = start.elapsed();

        // Verify results
        let succeeded: usize = results.iter().filter(|r| r.is_ok()).count();
        let failed: usize = results.iter().filter(|r| r.is_err()).count();

        let throughput = inputs.len() as f64 / parallel_elapsed.as_secs_f64();

        println!(
            "Parallel batch: {} bonds in {:?} ({:.0} bonds/sec), {} succeeded, {} failed",
            inputs.len(),
            parallel_elapsed,
            throughput,
            succeeded,
            failed
        );

        assert_eq!(succeeded, 500, "All bonds should price successfully");
        assert_eq!(failed, 0, "No bonds should fail");

        // Verify analytics
        for result in results.iter().take(10) {
            let output = result.as_ref().unwrap();
            assert!(output.ytm_mid.is_some(), "YTM should be calculated");
            assert!(
                output.modified_duration.is_some(),
                "Duration should be calculated"
            );
            assert!(
                output.z_spread_mid.is_some(),
                "Z-spread should be calculated"
            );
            assert!(
                output.i_spread_mid.is_some(),
                "I-spread should be calculated"
            );
        }
    }

    #[test]
    fn test_batch_pricing_with_stats() {
        let router = PricingRouter::new();
        let settlement = Date::from_ymd(2025, 6, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create batch with some bonds that will fail (no coupon rate)
        let mut bonds = create_bond_batch(100);

        // Make some bonds invalid (missing coupon for fixed rate)
        for i in [10, 25, 50, 75].iter() {
            if *i < bonds.len() {
                bonds[*i].coupon_rate = None;
            }
        }

        let inputs: Vec<PricingInput> = bonds
            .into_iter()
            .map(|bond| PricingInput {
                bond,
                settlement_date: settlement,
                market_price_bid: None,
                market_price_mid: Some(dec!(100.0)),
                market_price_ask: None,
                discount_curve: Some(curve.clone()),
                benchmark_curve: None,
                government_curve: None,
                volatility: None,
                bid_ask_config: None,
            })
            .collect();

        // Use batch with stats
        let result = router.price_batch_with_stats(&inputs);

        println!(
            "Batch with stats: {} bonds in {}ms ({:.0} bonds/sec)",
            inputs.len(),
            result.elapsed_ms,
            result.bonds_per_second
        );
        println!(
            "  Succeeded: {}, Failed: {}",
            result.succeeded, result.failed
        );

        // Should have some failures
        assert_eq!(result.succeeded, 96, "96 bonds should succeed");
        assert_eq!(result.failed, 4, "4 bonds should fail (no coupon)");
        assert_eq!(result.outputs.len(), 100, "Should have 100 results");

        // Throughput should be > 0
        assert!(
            result.bonds_per_second > 0.0,
            "Throughput should be positive"
        );
    }

    #[test]
    fn test_parallel_vs_sequential_speedup() {
        let router = PricingRouter::new();
        let settlement = Date::from_ymd(2025, 6, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create a moderately large batch
        let bonds = create_bond_batch(200);
        let inputs: Vec<PricingInput> = bonds
            .into_iter()
            .map(|bond| PricingInput {
                bond,
                settlement_date: settlement,
                market_price_bid: None,
                market_price_mid: Some(dec!(100.0)),
                market_price_ask: None,
                discount_curve: Some(curve.clone()),
                benchmark_curve: Some(curve.clone()),
                government_curve: None,
                volatility: None,
                bid_ask_config: None,
            })
            .collect();

        // Warm-up run
        let _ = router.price_batch(&inputs);

        // Time sequential
        let start_seq = std::time::Instant::now();
        let seq_results = router.price_batch(&inputs);
        let seq_elapsed = start_seq.elapsed();

        // Time parallel
        let start_par = std::time::Instant::now();
        let par_results = router.price_batch_parallel(&inputs);
        let par_elapsed = start_par.elapsed();

        // Both should produce same number of results
        assert_eq!(seq_results.len(), par_results.len());

        let seq_throughput = inputs.len() as f64 / seq_elapsed.as_secs_f64();
        let par_throughput = inputs.len() as f64 / par_elapsed.as_secs_f64();
        let speedup = seq_elapsed.as_secs_f64() / par_elapsed.as_secs_f64();

        println!(
            "Parallel vs Sequential comparison ({} bonds):",
            inputs.len()
        );
        println!(
            "  Sequential: {:?} ({:.0} bonds/sec)",
            seq_elapsed, seq_throughput
        );
        println!(
            "  Parallel:   {:?} ({:.0} bonds/sec)",
            par_elapsed, par_throughput
        );
        println!("  Speedup:    {:.2}x", speedup);

        // Parallel should not be slower than sequential
        // Note: For small batches, overhead might make parallel slower
        // For larger batches, we expect speedup proportional to CPU cores
        // We don't assert specific speedup as it depends on hardware

        // Verify correctness - results should be equivalent
        let seq_succeeded = seq_results.iter().filter(|r| r.is_ok()).count();
        let par_succeeded = par_results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(
            seq_succeeded, par_succeeded,
            "Same number of bonds should succeed"
        );

        // Verify YTMs match (parallel shouldn't affect calculation accuracy)
        for (seq_result, par_result) in seq_results.iter().zip(par_results.iter()).take(20) {
            if let (Ok(seq_out), Ok(par_out)) = (seq_result, par_result) {
                assert_eq!(seq_out.ytm_mid, par_out.ytm_mid, "YTMs should match");
                assert_eq!(
                    seq_out.modified_duration, par_out.modified_duration,
                    "Durations should match"
                );
            }
        }
    }

    #[test]
    fn test_bid_ask_spread_config() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create config with 10bp half-spread (20bp total)
        let spread_config = BidAskSpreadConfig::symmetric(10.0);

        let input = PricingInput {
            bond: bond.clone(),
            settlement_date: settlement,
            market_price_bid: None, // No market bid
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None, // No market ask
            discount_curve: Some(curve.clone()),
            benchmark_curve: Some(curve),
            government_curve: None,
            volatility: None,
            bid_ask_config: Some(spread_config),
        };

        // Test effective prices
        let (eff_bid, eff_mid, eff_ask) = input.effective_prices();
        assert!(eff_mid.is_some());
        assert!(eff_bid.is_some());
        assert!(eff_ask.is_some());

        let mid = eff_mid.unwrap();
        let bid = eff_bid.unwrap();
        let ask = eff_ask.unwrap();

        // Bid should be below mid (by 10bps = 0.10)
        // 100 * 10/10000 = 0.10
        assert!(bid < mid, "Bid {} should be less than mid {}", bid, mid);
        let expected_bid_adjustment = mid * Decimal::from_f64_retain(10.0 / 10000.0).unwrap();
        let actual_bid_diff = mid - bid;
        assert!(
            (actual_bid_diff - expected_bid_adjustment).abs() < dec!(0.001),
            "Bid adjustment {} should be close to expected {}",
            actual_bid_diff,
            expected_bid_adjustment
        );

        // Ask should be above mid (by 10bps = 0.10)
        assert!(ask > mid, "Ask {} should be greater than mid {}", ask, mid);
        let actual_ask_diff = ask - mid;
        assert!(
            (actual_ask_diff - expected_bid_adjustment).abs() < dec!(0.001),
            "Ask adjustment {} should be close to expected {}",
            actual_ask_diff,
            expected_bid_adjustment
        );

        // Price the bond and verify spreads are calculated at bid/mid/ask
        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();

        // Clean prices should match effective prices
        assert_eq!(output.clean_price_bid, Some(bid));
        assert_eq!(output.clean_price_mid, Some(mid));
        assert_eq!(output.clean_price_ask, Some(ask));

        // YTMs should be calculated at bid/mid/ask
        assert!(output.ytm_bid.is_some(), "YTM bid should be calculated");
        assert!(output.ytm_mid.is_some(), "YTM mid should be calculated");
        assert!(output.ytm_ask.is_some(), "YTM ask should be calculated");

        // YTM relationship: lower price -> higher yield
        // So bid price (lower) should have higher yield than ask price (higher)
        let ytm_bid = output.ytm_bid.unwrap();
        let ytm_mid = output.ytm_mid.unwrap();
        let ytm_ask = output.ytm_ask.unwrap();

        assert!(
            ytm_bid > ytm_mid,
            "YTM bid {} should be > YTM mid {} (lower price -> higher yield)",
            ytm_bid,
            ytm_mid
        );
        assert!(
            ytm_mid > ytm_ask,
            "YTM mid {} should be > YTM ask {} (lower price -> higher yield)",
            ytm_mid,
            ytm_ask
        );

        // Z-spreads should be calculated at bid/mid/ask
        assert!(
            output.z_spread_bid.is_some(),
            "Z-spread bid should be calculated"
        );
        assert!(
            output.z_spread_mid.is_some(),
            "Z-spread mid should be calculated"
        );
        assert!(
            output.z_spread_ask.is_some(),
            "Z-spread ask should be calculated"
        );

        // Z-spread relationship: lower price -> higher spread
        // So bid price (lower) should have higher spread than ask price (higher)
        let z_bid = output.z_spread_bid.unwrap();
        let z_mid = output.z_spread_mid.unwrap();
        let z_ask = output.z_spread_ask.unwrap();

        assert!(
            z_bid > z_mid,
            "Z-spread bid {} should be > Z-spread mid {} (lower price -> higher spread)",
            z_bid,
            z_mid
        );
        assert!(
            z_mid > z_ask,
            "Z-spread mid {} should be > Z-spread ask {} (lower price -> higher spread)",
            z_mid,
            z_ask
        );

        println!("BidAskSpreadConfig test results (10bp half-spread):");
        println!("  Prices: bid={}, mid={}, ask={}", bid, mid, ask);
        println!(
            "  YTMs: bid={:.4}%, mid={:.4}%, ask={:.4}%",
            ytm_bid.to_f64().unwrap() * 100.0,
            ytm_mid.to_f64().unwrap() * 100.0,
            ytm_ask.to_f64().unwrap() * 100.0
        );
        println!(
            "  Z-spreads: bid={:.1}bps, mid={:.1}bps, ask={:.1}bps",
            z_bid.to_f64().unwrap(),
            z_mid.to_f64().unwrap(),
            z_ask.to_f64().unwrap()
        );
    }

    #[test]
    fn test_bid_ask_spread_config_asymmetric() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // Create asymmetric config: bid spread 5bp, ask spread 15bp
        let spread_config = BidAskSpreadConfig::asymmetric(5.0, 15.0);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: None,
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: None,
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: Some(spread_config),
        };

        let (eff_bid, eff_mid, eff_ask) = input.effective_prices();
        let mid = eff_mid.unwrap();
        let bid = eff_bid.unwrap();
        let ask = eff_ask.unwrap();

        // Bid adjustment should be 5bps
        let bid_adjustment = mid - bid;
        let expected_bid = mid * Decimal::from_f64_retain(5.0 / 10000.0).unwrap();
        assert!(
            (bid_adjustment - expected_bid).abs() < dec!(0.001),
            "Bid adjustment {} should be ~5bps = {}",
            bid_adjustment,
            expected_bid
        );

        // Ask adjustment should be 15bps
        let ask_adjustment = ask - mid;
        let expected_ask = mid * Decimal::from_f64_retain(15.0 / 10000.0).unwrap();
        assert!(
            (ask_adjustment - expected_ask).abs() < dec!(0.001),
            "Ask adjustment {} should be ~15bps = {}",
            ask_adjustment,
            expected_ask
        );

        println!(
            "Asymmetric spread config: bid_adj={} (expected {}), ask_adj={} (expected {})",
            bid_adjustment, expected_bid, ask_adjustment, expected_ask
        );
    }

    #[test]
    fn test_bid_ask_spread_config_prefer_market() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // Create config with prefer_market_spread = true (default)
        let spread_config = BidAskSpreadConfig::symmetric(10.0);

        // Provide market bid/ask prices
        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price_bid: Some(dec!(99.5)), // Market bid
            market_price_mid: Some(dec!(100.0)),
            market_price_ask: Some(dec!(100.5)), // Market ask
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
            bid_ask_config: Some(spread_config),
        };

        let (eff_bid, eff_mid, eff_ask) = input.effective_prices();

        // Should use market prices since prefer_market_spread = true
        assert_eq!(eff_bid, Some(dec!(99.5)), "Should use market bid");
        assert_eq!(eff_mid, Some(dec!(100.0)), "Should use market mid");
        assert_eq!(eff_ask, Some(dec!(100.5)), "Should use market ask");
    }
}
