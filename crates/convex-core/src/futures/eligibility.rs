use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};

use super::{invalid, terms::*, validation::amount, FuturesReferenceError};
use crate::contracts::{BusinessDate, ExactDecimal, ReferenceDataId, UtcTimestamp};

/// Supplied economic time and information cutoff. Later restatements remain explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EligibilityContext {
    /// Economic instant of the proposed input use.
    pub as_of: UtcTimestamp,
    /// Latest permitted information availability.
    pub known_at: UtcTimestamp,
}

/// Stable reasons why a reference/cash dependency cannot support executable output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HedgeInputBlock {
    /// Fabricated facts.
    Synthetic,
    /// Model/informational facts.
    Indicative,
    /// Archived indicative-only facts.
    Historical,
    /// Exclusive data-validity cutoff has been reached.
    Stale,
    /// Original assertion is not yet economically effective.
    NotYetEffective,
    /// Assertion was not available at the supplied information cutoff.
    NotYetKnown,
    /// Source does not attest the complete delivery universe.
    IncompleteBasket,
    /// Before listing or on/after the conservatively closed last trading day.
    OutsideTradingWindow,
    /// Side-specific actual bid/ask is absent.
    MissingQuote,
    /// Side-specific available face is absent or insufficient.
    InsufficientSize,
    /// Proposed face does not respect the exact positive lot increment.
    OffLot,
    /// Missing security-specific funding.
    MissingFinancing,
    /// Funding availability does not cover the supplied holding/delivery horizon.
    FinancingHorizon,
    /// Explicit shorting/borrow permission is absent or insufficient.
    MissingBorrow,
    /// Borrow availability ends before the supplied horizon.
    BorrowHorizon,
    /// Separate actual cost inputs are absent.
    MissingCosts,
    /// Quoted settlement precedes the economic date.
    PastSettlement,
}

/// One failed prerequisite, retaining the affected original resource identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HedgeInputIssue {
    /// Original resource identity, not an invented fallback candidate.
    pub resource: ReferenceDataId,
    /// Stable blocking reason.
    pub reason: HedgeInputBlock,
}

/// Necessary reference/data admission only. It is neither order authority nor a hedge proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HedgeInputEligibility {
    /// True only if every checked prerequisite passes; later risk/execution gates still apply.
    pub eligible: bool,
    /// All diagnosed reasons, deterministic by original resource and reason.
    pub issues: Vec<HedgeInputIssue>,
}

impl HedgeInputEligibility {
    fn block(&mut self, stamp: &ReferenceStamp, reason: HedgeInputBlock) {
        self.issues.push(HedgeInputIssue {
            resource: stamp.id.clone(),
            reason,
        });
    }
    fn check_stamp(&mut self, stamp: &ReferenceStamp, context: EligibilityContext) {
        let reason = match stamp.quality {
            ReferenceQuality::Synthetic => Some(HedgeInputBlock::Synthetic),
            ReferenceQuality::Indicative => Some(HedgeInputBlock::Indicative),
            ReferenceQuality::Historical => Some(HedgeInputBlock::Historical),
            ReferenceQuality::Exchange | ReferenceQuality::Provider => None,
        };
        if let Some(reason) = reason {
            self.block(stamp, reason);
        }
        if stamp.available_at > context.known_at {
            self.block(stamp, HedgeInputBlock::NotYetKnown);
        }
        if stamp.source_at > context.as_of {
            self.block(stamp, HedgeInputBlock::NotYetEffective);
        }
        if stamp.valid_until <= context.as_of {
            self.block(stamp, HedgeInputBlock::Stale);
        }
    }
    fn finish(mut self) -> Self {
        self.issues.sort();
        self.issues.dedup();
        self.eligible = self.issues.is_empty();
        self
    }
}

fn report() -> HedgeInputEligibility {
    HedgeInputEligibility {
        eligible: false,
        issues: Vec::new(),
    }
}

/// Check exact disjoint contract/basket identity and every supplied freshness/classification gate.
/// Rates reject any basket. Bond contracts require their exact full basket, even for indicative use.
pub fn assess_futures_reference(
    contract: &FuturesContract,
    basket: Option<&DeliveryBasket>,
    context: EligibilityContext,
) -> Result<HedgeInputEligibility, FuturesReferenceError> {
    contract.validate()?;
    let mut result = report();
    result.check_stamp(&contract.stamp, context);
    let date = BusinessDate::try_from(crate::Date::from(context.as_of.datetime().date_naive()))?;
    if date < contract.listed_from || date >= contract.last_trade {
        result.block(&contract.stamp, HedgeInputBlock::OutsideTradingWindow);
    }
    match &contract.settlement {
        FuturesSettlement::Bond { .. } => {
            let basket = basket
                .ok_or_else(|| invalid("basket", "exact bond delivery basket is required"))?;
            basket.validate_contract(contract)?;
            result.check_stamp(&basket.stamp, context);
            if !basket.complete {
                result.block(&basket.stamp, HedgeInputBlock::IncompleteBasket);
            }
        }
        FuturesSettlement::Rate { .. } if basket.is_some() => {
            return Err(invalid(
                "basket",
                "rate futures cannot consume bond basket/CTD terms",
            ));
        }
        FuturesSettlement::Rate { .. } => {}
    }
    Ok(result.finish())
}

/// Proposed cash direction. Selling existing owned face need not claim borrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CashHedgeSide {
    /// Purchase at the actual ask.
    Buy,
    /// Sale at the actual bid; caller must supply owned face.
    Sell,
    /// Short sale at the actual bid with mandatory explicit borrow availability.
    Short,
}

/// Actual face and holding horizon for one candidate; quantities never mean contract counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashHedgeRequest {
    /// Side with separate shorting semantics.
    pub side: CashHedgeSide,
    /// Exact strictly positive proposed face.
    pub face: ExactDecimal,
    /// Existing owned face, nonnegative. Does not infer a position from a quote.
    pub owned_face: ExactDecimal,
    /// Last required funding/borrowing date, inclusive.
    pub horizon: BusinessDate,
}

/// Check actual side-specific cash quote/size, exact lots, funding, borrow and costs.
/// No candidate is dropped and no missing price/size/rate becomes zero.
pub fn assess_cash_hedge(
    universe: &CashHedgeUniverse,
    instrument: &InstrumentPin,
    request: CashHedgeRequest,
    context: EligibilityContext,
) -> Result<HedgeInputEligibility, FuturesReferenceError> {
    universe.validate()?;
    amount("requested_face", request.face, true)?;
    amount("owned_face", request.owned_face, false)?;
    let candidate = universe
        .candidates
        .iter()
        .find(|c| &c.instrument == instrument)
        .ok_or_else(|| {
            invalid(
                "cash_candidate",
                "exact candidate identity/version is absent",
            )
        })?;
    if request.horizon < candidate.settlement {
        return Err(invalid(
            "holding_horizon",
            "horizon precedes quoted settlement",
        ));
    }
    let mut result = report();
    result.check_stamp(&universe.stamp, context);
    result.check_stamp(&candidate.quote, context);
    let quote = if request.side == CashHedgeSide::Buy {
        candidate.ask_per100
    } else {
        candidate.bid_per100
    };
    let size = if request.side == CashHedgeSide::Buy {
        candidate.ask_size_face
    } else {
        candidate.bid_size_face
    };
    if quote.is_none() {
        result.block(&candidate.quote, HedgeInputBlock::MissingQuote);
    }
    if size.is_none_or(|s| s < request.face) {
        result.block(&candidate.quote, HedgeInputBlock::InsufficientSize);
    }
    if request.face.decimal() % candidate.lot_face.decimal() != Decimal::ZERO {
        result.block(&candidate.quote, HedgeInputBlock::OffLot);
    }
    if request.side == CashHedgeSide::Sell && request.face > request.owned_face {
        result.block(&candidate.quote, HedgeInputBlock::InsufficientSize);
    }
    let date = BusinessDate::try_from(crate::Date::from(context.as_of.datetime().date_naive()))?;
    if candidate.settlement < date {
        result.block(&candidate.quote, HedgeInputBlock::PastSettlement);
    }
    check_financing(candidate, request, context, &mut result);
    Ok(result.finish())
}

fn check_financing(
    candidate: &CashHedgeCandidate,
    request: CashHedgeRequest,
    context: EligibilityContext,
    result: &mut HedgeInputEligibility,
) {
    if let Some(funding) = &candidate.financing {
        result.check_stamp(&funding.stamp, context);
        if funding.end < request.horizon {
            result.block(&funding.stamp, HedgeInputBlock::FinancingHorizon);
        }
    } else {
        result.block(&candidate.quote, HedgeInputBlock::MissingFinancing);
    }
    if let Some(costs) = &candidate.costs {
        result.check_stamp(&costs.stamp, context);
    } else {
        result.block(&candidate.quote, HedgeInputBlock::MissingCosts);
    }
    if request.side == CashHedgeSide::Short {
        if let Some(borrow) = &candidate.borrow {
            result.check_stamp(&borrow.stamp, context);
            if borrow.available_face < request.face {
                result.block(&borrow.stamp, HedgeInputBlock::MissingBorrow);
            }
            if borrow.until < request.horizon {
                result.block(&borrow.stamp, HedgeInputBlock::BorrowHorizon);
            }
        } else {
            result.block(&candidate.quote, HedgeInputBlock::MissingBorrow);
        }
    }
}

impl FuturesContract {
    /// Exact outright tick for a supplied trading date, never a hard-coded root-symbol default.
    pub fn tick_at(&self, date: BusinessDate) -> Result<ExactDecimal, FuturesReferenceError> {
        self.validate()?;
        if date < self.listed_from || date > self.last_trade {
            return Err(invalid("tick_date", "date outside actual trading life"));
        }
        self.ticks
            .iter()
            .find(|tick| date >= tick.from && tick.until.is_none_or(|end| date < end))
            .map(|tick| tick.increment)
            .ok_or_else(|| invalid("tick", "missing dated tick regime"))
    }

    /// Validate bounded signed integer outright contracts and exact price ticks. Never round orders.
    pub fn validate_order_units(
        &self,
        date: BusinessDate,
        contracts: i64,
        price_points: ExactDecimal,
    ) -> Result<(), FuturesReferenceError> {
        let tick = self.tick_at(date)?;
        if contracts == 0 || contracts.unsigned_abs() > 1_000_000_000 {
            return Err(invalid(
                "contracts",
                "expected nonzero integer contracts within1billion",
            ));
        }
        if price_points.decimal() <= Decimal::ZERO
            || price_points.decimal() > Decimal::from(1000)
            || price_points.decimal() % tick.decimal() != Decimal::ZERO
        {
            return Err(invalid(
                "price_points",
                "price is out of bounds or off the exact dated tick",
            ));
        }
        if matches!(self.settlement, FuturesSettlement::Rate { .. })
            && price_points.decimal() > Decimal::from(200)
        {
            return Err(invalid("rate_quote", "rate index points outside[0,200]"));
        }
        Ok(())
    }

    /// Decode100-minus-annual-percent rate to a decimal rate; no convexity is silently added.
    pub fn rate_from_price(
        &self,
        price_points: ExactDecimal,
    ) -> Result<ExactDecimal, FuturesReferenceError> {
        self.validate()?;
        if !matches!(self.settlement, FuturesSettlement::Rate { .. }) {
            return Err(invalid(
                "rate_quote",
                "bond futures do not have100-minus-rate quotations",
            ));
        }
        if price_points.decimal() < Decimal::ZERO || price_points.decimal() > Decimal::from(200) {
            return Err(invalid("rate_quote", "rate index points outside[0,200]"));
        }
        Ok(ExactDecimal::new(
            (Decimal::from(100) - price_points.decimal()) / Decimal::from(100),
        ))
    }

    /// Add the explicit declared futures-to-forward adjustment; absent admission fails.
    pub fn calibration_rate(
        &self,
        price_points: ExactDecimal,
    ) -> Result<ExactDecimal, FuturesReferenceError> {
        let rate = self.rate_from_price(price_points)?.decimal();
        let FuturesSettlement::Rate { convexity, .. } = &self.settlement else {
            return Err(invalid(
                "calibration",
                "bond futures require their full delivery economics",
            ));
        };
        match convexity {
            FuturesConvexityPolicy::NotAdmitted => Err(invalid(
                "convexity",
                "no calibration adjustment policy admitted",
            )),
            FuturesConvexityPolicy::ZeroAssumption { .. } => Ok(ExactDecimal::new(rate)),
            FuturesConvexityPolicy::Supplied { decimal_rate, .. } => rate
                .checked_add(decimal_rate.decimal())
                .map(ExactDecimal::new)
                .ok_or_else(|| invalid("convexity", "adjustment overflow")),
        }
    }
}

/// One physical-delivery invoice in contract currency using the exact pinned published factor.
/// Converted clean component is rounded per lot before adding already-rounded supplied accrued cash.
/// This arithmetic neither computes accrued interest nor selects CTD or records realized settlement.
pub fn bond_delivery_invoice(
    contract: &FuturesContract,
    basket: &DeliveryBasket,
    bond: &InstrumentPin,
    delivery_date: BusinessDate,
    settlement_price_points: ExactDecimal,
    accrued_currency_per_contract: ExactDecimal,
) -> Result<ExactDecimal, FuturesReferenceError> {
    basket.validate_contract(contract)?;
    let FuturesSettlement::Bond { delivery, .. } = &contract.settlement else {
        return Err(invalid(
            "invoice",
            "rate futures have no physical bond invoice",
        ));
    };
    if delivery_date < delivery.first_delivery || delivery_date > delivery.last_delivery {
        return Err(invalid(
            "delivery_date",
            "date outside actual delivery interval",
        ));
    }
    let factor = basket
        .bonds
        .iter()
        .find(|entry| &entry.instrument == bond)
        .ok_or_else(|| {
            invalid(
                "deliverable",
                "exact bond revision is not in the eligible basket",
            )
        })?
        .conversion_factor
        .decimal();
    amount("settlement_price", settlement_price_points, true)?;
    amount("accrued_currency", accrued_currency_per_contract, false)?;
    if settlement_price_points.decimal() > Decimal::from(1000) {
        return Err(invalid("settlement_price", "price exceeds1000points"));
    }
    let places = contract.daily_settlement.cash_decimal_places;
    let accrued = accrued_currency_per_contract.decimal();
    if accrued.round_dp(places) != accrued {
        return Err(invalid(
            "accrued_currency",
            "supplied accrued cash must respect posting precision",
        ));
    }
    let clean = contract
        .currency_per_price_point
        .decimal()
        .checked_mul(settlement_price_points.decimal())
        .and_then(|v| v.checked_mul(factor))
        .ok_or_else(|| invalid("invoice", "converted clean amount overflow"))?
        .round_dp_with_strategy(places, RoundingStrategy::MidpointAwayFromZero);
    clean
        .checked_add(accrued)
        .map(ExactDecimal::new)
        .ok_or_else(|| invalid("invoice", "invoice cash overflow"))
}
