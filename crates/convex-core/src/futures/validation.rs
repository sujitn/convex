use std::collections::BTreeSet;

use rust_decimal::Decimal;

use super::{invalid, terms::*, FuturesReferenceError};
use crate::contracts::{BusinessDate, ExactDecimal};
use crate::daycounts::DayCountConvention;

pub(crate) fn text(value: &str) -> Result<(), FuturesReferenceError> {
    if value.trim().is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(invalid("reference_text", "expected bounded nonempty text"));
    }
    Ok(())
}

pub(crate) fn amount(
    field: &'static str,
    value: ExactDecimal,
    positive: bool,
) -> Result<(), FuturesReferenceError> {
    let decimal = value.decimal();
    if decimal < Decimal::ZERO
        || (positive && decimal == Decimal::ZERO)
        || decimal > Decimal::from(1_000_000_000_000_000_u64)
    {
        return Err(invalid(
            field,
            "amount outside declared positive/nonnegative bound",
        ));
    }
    Ok(())
}

fn interval(start: BusinessDate, end: BusinessDate) -> Result<(), FuturesReferenceError> {
    if start >= end || start.date().days_between(&end.date()) > 36_600 {
        return Err(invalid(
            "date_interval",
            "expected increasing interval within100years",
        ));
    }
    Ok(())
}

fn rate_basis(basis: DayCountConvention) -> Result<(), FuturesReferenceError> {
    if !matches!(
        basis,
        DayCountConvention::Act360 | DayCountConvention::Act365Fixed
    ) {
        return Err(invalid(
            "day_count",
            "reference rate requires ACT/360 or ACT/365F",
        ));
    }
    Ok(())
}

impl ReferenceStamp {
    /// Validate explicit time ordering, evidence and bounded validity before use.
    pub fn validate(&self) -> Result<(), FuturesReferenceError> {
        text(&self.evidence)?;
        if self.available_at < self.source_at || self.valid_until <= self.available_at {
            return Err(invalid(
                "reference_stamp",
                "invalid source/availability/validity ordering",
            ));
        }
        Ok(())
    }
}

impl ContractMonth {
    /// Validate named month without deriving settlement or observation dates.
    pub fn validate(self) -> Result<(), FuturesReferenceError> {
        if !(1900..=2200).contains(&self.year) || !(1..=12).contains(&self.month) {
            return Err(invalid(
                "contract_month",
                "unsupported named exchange month",
            ));
        }
        Ok(())
    }
}

impl RateAccrual {
    /// Return the actual half-open value/observation interval.
    pub fn window(&self) -> (BusinessDate, BusinessDate) {
        match self {
            Self::Overnight { start, end, .. } | Self::Term { start, end, .. } => (*start, *end),
        }
    }
    /// Validate rate-family semantics; no bond CTD input is applicable.
    pub fn validate(&self) -> Result<(), FuturesReferenceError> {
        let (start, end) = self.window();
        interval(start, end)?;
        if start.date().days_between(&end.date()) > 366 {
            return Err(invalid("rate_window", "rate accrual window exceeds366days"));
        }
        let basis = match self {
            Self::Overnight { day_count, .. } => *day_count,
            Self::Term {
                fixing, day_count, ..
            } => {
                if *fixing > start || fixing.date().days_between(&start.date()) > 31 {
                    return Err(invalid(
                        "fixing",
                        "term fixing must precede value start by0..31days",
                    ));
                }
                *day_count
            }
        };
        rate_basis(basis)
    }
}

impl FuturesConvexityPolicy {
    /// Validate a supplied calibration adjustment without selecting an implicit assumption.
    pub fn validate(&self) -> Result<(), FuturesReferenceError> {
        match self {
            Self::NotAdmitted => Ok(()),
            Self::ZeroAssumption { evidence } => text(evidence),
            Self::Supplied {
                decimal_rate,
                evidence,
                ..
            } => {
                if decimal_rate.decimal().abs() > Decimal::ONE {
                    return Err(invalid("convexity", "adjustment exceeds one decimal rate"));
                }
                text(evidence)
            }
        }
    }
}

impl FuturesContract {
    /// Check complete quotation units, dated tick coverage and disjoint contract semantics.
    pub fn validate(&self) -> Result<(), FuturesReferenceError> {
        self.stamp.validate()?;
        if self.instrument_id.as_str() != self.stamp.id.as_str() {
            return Err(invalid(
                "instrument_id",
                "contract stamp identity must match instrument",
            ));
        }
        self.contract_month.validate()?;
        text(&self.product_code)?;
        text(&self.contract_code)?;
        text(&self.daily_settlement.time_zone)?;
        interval(self.listed_from, self.last_trade)?;
        if self.expiry < self.last_trade || self.daily_settlement.cash_decimal_places > 8 {
            return Err(invalid("settlement", "invalid expiry or cash precision"));
        }
        amount("point_multiplier", self.currency_per_price_point, true)?;
        if self.currency_per_price_point.decimal() > Decimal::from(1_000_000_000_u64) {
            return Err(invalid(
                "point_multiplier",
                "multiplier exceeds1billion currency per point",
            ));
        }
        self.validate_ticks()?;
        self.validate_settlement()
    }

    fn validate_ticks(&self) -> Result<(), FuturesReferenceError> {
        if self.ticks.is_empty() || self.ticks.len() > 32 {
            return Err(invalid("ticks", "expected1..32dated tick regimes"));
        }
        let mut next = Some(self.listed_from);
        for tick in &self.ticks {
            if next != Some(tick.from)
                || tick.until.is_some_and(|end| end <= tick.from)
                || tick.increment.decimal() <= Decimal::ZERO
                || tick.increment.decimal() > Decimal::ONE
            {
                return Err(invalid(
                    "ticks",
                    "noncontiguous, unordered or invalid tick regime",
                ));
            }
            next = tick.until;
        }
        if self
            .ticks
            .last()
            .is_none_or(|tick| tick.from > self.last_trade)
            || next.is_some_and(|end| end <= self.last_trade)
        {
            return Err(invalid(
                "ticks",
                "tick regimes do not cover full trading life",
            ));
        }
        Ok(())
    }

    fn validate_settlement(&self) -> Result<(), FuturesReferenceError> {
        match &self.settlement {
            FuturesSettlement::Bond {
                contract_face,
                delivery,
            } => {
                amount("contract_face", *contract_face, true)?;
                let expected = contract_face
                    .decimal()
                    .checked_div(Decimal::from(100))
                    .ok_or_else(|| invalid("contract_face", "face conversion overflow"))?;
                if expected != self.currency_per_price_point.decimal()
                    || delivery.first_notice > delivery.first_delivery
                    || delivery.first_delivery > delivery.last_delivery
                    || delivery.first_notice < self.listed_from
                    || self.last_trade > delivery.last_delivery
                    || self.expiry > delivery.last_delivery
                    || delivery
                        .last_delivery
                        .date()
                        .days_between(&delivery.first_delivery.date())
                        .abs()
                        > 62
                {
                    return Err(invalid(
                        "bond_delivery",
                        "incompatible face/multiplier or critical dates",
                    ));
                }
                Ok(())
            }
            FuturesSettlement::Rate {
                final_settlement,
                accrual,
                convexity,
            } => {
                accrual.validate()?;
                convexity.validate()?;
                let (_, end) = accrual.window();
                let rate_final = match accrual {
                    RateAccrual::Overnight { .. } => end,
                    RateAccrual::Term { fixing, .. } => *fixing,
                };
                if *final_settlement < self.expiry
                    || *final_settlement < rate_final
                    || rate_final.date().days_between(&final_settlement.date()) > 31
                    || self.last_trade > rate_final
                {
                    return Err(invalid(
                        "rate_settlement",
                        "incompatible accrual, expiry or settlement dates",
                    ));
                }
                Ok(())
            }
        }
    }
}

impl DeliveryBasket {
    /// Validate a bounded unique actual security universe and positive published factors.
    pub fn validate(&self) -> Result<(), FuturesReferenceError> {
        self.stamp.validate()?;
        self.contract_month.validate()?;
        if self.bonds.is_empty() || self.bonds.len() > 128 {
            return Err(invalid("basket", "expected1..128deliverable bonds"));
        }
        let mut ids = BTreeSet::new();
        for bond in &self.bonds {
            if !ids.insert(&bond.instrument.id)
                || bond.conversion_factor.decimal() <= Decimal::ZERO
                || bond.conversion_factor.decimal() > Decimal::from(10)
            {
                return Err(invalid(
                    "basket",
                    "duplicate security or conversion factor outside(0,10]",
                ));
            }
        }
        Ok(())
    }

    /// Check both exact reciprocal revisions, named month and currency; no latest fallback.
    pub fn validate_contract(
        &self,
        contract: &FuturesContract,
    ) -> Result<(), FuturesReferenceError> {
        self.validate()?;
        contract.validate()?;
        let FuturesSettlement::Bond { delivery, .. } = &contract.settlement else {
            return Err(invalid(
                "basket",
                "rate futures have no delivery basket/CTD",
            ));
        };
        if self.contract.id != contract.instrument_id
            || self.contract.version != contract.stamp.version
            || self.contract_month != contract.contract_month
            || self.currency != contract.currency
            || delivery.basket.id != self.stamp.id
            || delivery.basket.version != self.stamp.version
        {
            return Err(invalid(
                "basket",
                "basket and contract exact identities/versions/month/currency differ",
            ));
        }
        Ok(())
    }
}

impl FinancingInput {
    /// Validate bounded explicit simple funding; no rate curve or flat-rate fallback.
    pub fn validate(&self) -> Result<(), FuturesReferenceError> {
        self.stamp.validate()?;
        interval(self.start, self.end)?;
        rate_basis(self.day_count)?;
        if self.repo_decimal.decimal().abs() > Decimal::ONE
            || self.haircut_fraction.decimal() < Decimal::ZERO
            || self.haircut_fraction.decimal() >= Decimal::ONE
        {
            return Err(invalid(
                "financing",
                "repo/haircut outside supplied reference bounds",
            ));
        }
        Ok(())
    }
}

impl CashHedgeCandidate {
    /// Validate quote/size/borrow/funding facts without manufacturing missing inputs.
    pub fn validate(&self) -> Result<(), FuturesReferenceError> {
        self.quote.validate()?;
        amount("lot_face", self.lot_face, true)?;
        for value in [self.bid_per100, self.ask_per100].into_iter().flatten() {
            if value.decimal() <= Decimal::ZERO || value.decimal() > Decimal::from(1000) {
                return Err(invalid("cash_quote", "price must be in(0,1000]per100face"));
            }
        }
        if self
            .bid_per100
            .zip(self.ask_per100)
            .is_some_and(|(bid, ask)| bid > ask)
        {
            return Err(invalid("cash_quote", "bid exceeds ask"));
        }
        for value in [self.bid_size_face, self.ask_size_face]
            .into_iter()
            .flatten()
        {
            amount("cash_size", value, false)?;
        }
        if let Some(funding) = &self.financing {
            funding.validate()?;
            if funding.instrument != self.instrument
                || funding.start != self.settlement
                || funding.currency != self.currency
            {
                return Err(invalid(
                    "financing",
                    "funding security/version/start differs from quote",
                ));
            }
        }
        if let Some(borrow) = &self.borrow {
            borrow.stamp.validate()?;
            rate_basis(borrow.day_count)?;
            amount("borrow_face", borrow.available_face, false)?;
            if borrow.fee_decimal.decimal() < Decimal::ZERO
                || borrow.fee_decimal.decimal() > Decimal::ONE
                || borrow.until < self.settlement
            {
                return Err(invalid("borrow", "invalid borrowing fee/date"));
            }
        }
        if let Some(cost) = &self.costs {
            cost.stamp.validate()?;
            amount("cost_per100", cost.currency_per100_face, false)?;
        }
        Ok(())
    }
}

impl CashHedgeUniverse {
    /// Validate1..128unique actual security candidates and one explicit currency.
    pub fn validate(&self) -> Result<(), FuturesReferenceError> {
        self.stamp.validate()?;
        if self.candidates.is_empty() || self.candidates.len() > 128 {
            return Err(invalid(
                "cash_universe",
                "expected1..128cash hedge candidates",
            ));
        }
        let mut ids = BTreeSet::new();
        for candidate in &self.candidates {
            candidate.validate()?;
            if candidate.currency != self.currency || !ids.insert(&candidate.instrument.id) {
                return Err(invalid(
                    "cash_universe",
                    "duplicate security or incompatible currency",
                ));
            }
        }
        Ok(())
    }
}
