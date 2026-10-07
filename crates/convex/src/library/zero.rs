//! Original complete zero/bill inputs and checked native quotation.
use super::inputs::ResolvedInputs;
use super::*;
use convex_bonds::{
    instruments::{Compounding as ZCompounding, ZeroCouponBond},
    types::{BondIdentifiers, Cusip, Figi, Isin, Sedol},
};
use convex_core::{
    contracts::ExactDecimal,
    daycounts::DayCountConvention,
    types::{Frequency, Mark},
};
use serde::{Deserialize, Serialize};

/// Original full identifier fields; strings reconstruct through native checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ZeroIdentifiers {
    /// Actual CUSIP, if supplied.
    pub cusip: Option<String>,
    /// Actual ISIN, if supplied.
    pub isin: Option<String>,
    /// Actual FIGI, if supplied.
    pub figi: Option<String>,
    /// Actual SEDOL, if supplied.
    pub sedol: Option<String>,
    /// Original ticker, without provider lookup.
    pub ticker: Option<String>,
    /// Original issuer name.
    pub issuer_name: Option<String>,
    /// Original issue name.
    pub issue_name: Option<String>,
}
/// Native zero compounding admits exactly these original five choices.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ZeroCompounding {
    /// Annual yield.
    Annual,
    /// Semiannual yield.
    SemiAnnual,
    /// Quarterly yield.
    Quarterly,
    /// Monthly yield.
    Monthly,
    /// Continuous yield.
    Continuous,
}
/// All original native zero/bill fields, retaining absolute redemption and face.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ZeroInput {
    /// Complete original instrument identifiers.
    pub identifiers: ZeroIdentifiers,
    /// Original issue date; no default to maturity.
    pub issue_date: Date,
    /// Actual original maturity date.
    pub maturity: Date,
    /// Original optional absolute issue price, never defaulted to par.
    pub issue_price: Option<ExactDecimal>,
    /// Explicit native quotation day count.
    pub day_count: DayCountConvention,
    /// Original explicit yield compounding.
    pub compounding: ZeroCompounding,
    /// Original settlement business-day lag, retained independently of quote date.
    pub settlement_days: u32,
    /// Actual original calendar revision.
    pub calendar: convex_rates::ReferencePin,
    /// Original currency.
    pub currency: Currency,
    /// Positive actual original face.
    pub face_value: ExactDecimal,
    /// Positive actual absolute redemption cash.
    pub redemption_value: ExactDecimal,
}
impl ZeroInput {
    pub(super) fn native(&self, r: &ResolvedInputs<'_>) -> LibraryResult<ZeroCouponBond> {
        r.calendar(&self.calendar)?;
        if self.issue_date >= self.maturity
            || !(1..=9999).contains(&self.issue_date.year())
            || !(1..=9999).contains(&self.maturity.year())
            || self.issue_date.days_between(&self.maturity)
                > convex_core::cashflows::MAX_CONTRACT_DAYS
            || self.face_value.decimal() <= rust_decimal::Decimal::ZERO
            || self.redemption_value.decimal() <= rust_decimal::Decimal::ZERO
            || self
                .issue_price
                .is_some_and(|p| p.decimal() <= rust_decimal::Decimal::ZERO)
            || self.settlement_days > 30
        {
            return Err(LibraryError::invalid(
                "zero terms",
                "positive bounded dated face/redemption/issue price and settlement lag required",
            ));
        }
        let mut ids = BondIdentifiers::new();
        for v in [
            &self.identifiers.ticker,
            &self.identifiers.issuer_name,
            &self.identifiers.issue_name,
        ]
        .into_iter()
        .flatten()
        {
            if v.is_empty() || v.len() > 512 || v.chars().any(char::is_control) {
                return Err(LibraryError::invalid(
                    "zero identifiers",
                    "bounded explicit labels required",
                ));
            }
        }
        if let Some(v) = &self.identifiers.cusip {
            ids = ids.with_cusip(
                Cusip::new(v).map_err(|e| LibraryError::invalid("cusip", &e.to_string()))?,
            );
        }
        if let Some(v) = &self.identifiers.isin {
            ids = ids.with_isin(
                Isin::new(v).map_err(|e| LibraryError::invalid("isin", &e.to_string()))?,
            );
        }
        if let Some(v) = &self.identifiers.figi {
            ids = ids.with_figi(
                Figi::new(v).map_err(|e| LibraryError::invalid("figi", &e.to_string()))?,
            );
        }
        if let Some(v) = &self.identifiers.sedol {
            ids = ids.with_sedol(
                Sedol::new(v).map_err(|e| LibraryError::invalid("sedol", &e.to_string()))?,
            );
        }
        if let Some(v) = &self.identifiers.ticker {
            ids = ids.with_ticker(v);
        }
        if let Some(v) = &self.identifiers.issuer_name {
            ids = ids.with_issuer_name(v);
        }
        if let Some(v) = &self.identifiers.issue_name {
            ids = ids.with_issue_name(v);
        }
        let c = match self.compounding {
            ZeroCompounding::Annual => ZCompounding::Annual,
            ZeroCompounding::SemiAnnual => ZCompounding::SemiAnnual,
            ZeroCompounding::Quarterly => ZCompounding::Quarterly,
            ZeroCompounding::Monthly => ZCompounding::Monthly,
            ZeroCompounding::Continuous => ZCompounding::Continuous,
        };
        let mut b = ZeroCouponBond::builder()
            .identifiers(ids)
            .issue_date(self.issue_date)
            .maturity(self.maturity)
            .day_count(self.day_count)
            .compounding(c)
            .settlement_days(self.settlement_days)
            .calendar(convex_bonds::types::CalendarId::new(
                self.calendar.id.as_str(),
            ))
            .currency(self.currency)
            .face_value(self.face_value.decimal())
            .redemption_value(self.redemption_value.decimal());
        if let Some(p) = self.issue_price {
            b = b.issue_price(p.decimal());
        }
        b.build()
            .map_err(|e| LibraryError::invalid("zero construction", &e.to_string()))
    }
}
/// Original applicable zero/bill routes with explicitly distinct price units.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ZeroOperation {
    /// Reconstruct original complete terms without deriving a price.
    Construct {},
    /// Native checked price/yield/risk for an actual per100 price.
    QuotePrice {
        /// Explicit active settlement.
        settlement: Date,
        /// Exact positive per100 original face.
        clean_price_per100: ExactDecimal,
    },
    /// Native checked price/yield/risk for an actual dated periodic yield.
    QuoteYield {
        /// Explicit active settlement.
        settlement: Date,
        /// Actual decimal annualized yield.
        decimal_yield: ExactDecimal,
        /// Explicit original mark compounding.
        frequency: Frequency,
    },
    /// Checked short-bill bank discount/BEY quotations, expressed in percent.
    BillConversions {
        /// Explicit active settlement.
        settlement: Date,
        /// Exact observed absolute price on original face.
        price_original_face: ExactDecimal,
    },
}
pub(super) fn zero(
    input: &ZeroInput,
    action: &ZeroOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let bond = input.native(r)?;
    let (settlement, mark) = match action {
        ZeroOperation::Construct {} => return value(input),
        ZeroOperation::QuotePrice {
            settlement,
            clean_price_per100,
        } => (
            *settlement,
            Mark::Price {
                value: clean_price_per100.decimal(),
                kind: convex_core::types::PriceKind::Clean,
            },
        ),
        ZeroOperation::QuoteYield {
            settlement,
            decimal_yield,
            frequency,
        } => (
            *settlement,
            Mark::Yield {
                value: decimal_yield.decimal(),
                frequency: *frequency,
            },
        ),
        ZeroOperation::BillConversions {
            settlement,
            price_original_face,
        } => {
            if *settlement < input.issue_date
                || *settlement >= input.maturity
                || price_original_face.decimal() <= rust_decimal::Decimal::ZERO
            {
                return Err(LibraryError::invalid(
                    "bill quote",
                    "active settlement and positive exact price required",
                ));
            }
            let days = settlement.days_between(&input.maturity);
            if days > 182 || input.face_value != input.redemption_value {
                return Err(LibraryError{code:LibraryErrorCode::Unsupported,message:"checked bill conversions require at most182 actual days and face redemption; use zero quote for other redemption economics".into(),details:Value::Null});
            }
            let p = price_original_face.decimal();
            // Guard retained Decimal kernels before their infallible arithmetic.
            let minimum = rust_decimal::Decimal::new(1, 15);
            let maximum = rust_decimal::Decimal::from(1_000_000_000_000_000i64);
            if p < minimum
                || p > maximum
                || input.face_value.decimal() < minimum
                || input.face_value.decimal() > maximum
            {
                return Err(LibraryError::limit(
                    "bill cash values outside retained bounded arithmetic domain",
                ));
            }
            let ratio = p
                .checked_div(input.face_value.decimal())
                .ok_or_else(|| LibraryError::limit("bill cash ratio overflow"))?;
            if ratio < rust_decimal::Decimal::new(1, 6)
                || ratio > rust_decimal::Decimal::from(1_000_000)
            {
                return Err(LibraryError::limit(
                    "bill cash ratio outside retained bounded arithmetic domain",
                ));
            }
            let discount = convex_analytics::yields::discount_yield(
                p,
                input.face_value.decimal(),
                days as u32,
            )
            .map_err(|e| LibraryError::invalid("bank discount quote", &e.to_string()))?;
            let equivalent = convex_analytics::yields::bond_equivalent_yield(
                p,
                input.face_value.decimal(),
                days as u32,
            )
            .map_err(|e| LibraryError::invalid("bill equivalent quote", &e.to_string()))?;
            return Ok(
                serde_json::json!({"terms":input,"settlement":settlement,"price_original_face":price_original_face,
                "discount_yield_percent":discount.to_string(),"bond_equivalent_yield_percent":equivalent.to_string()}),
            );
        }
    };
    let (p, q) = convex_analytics::bond_quote::quote_zero(&bond, settlement, &mark)
        .map_err(|e| LibraryError::invalid("zero quote", &e.to_string()))?;
    Ok(
        serde_json::json!({"terms":input,"settlement":settlement,"pricing":{"clean_price_per_100":p.clean_price_per_100,"dirty_price_per_100":p.dirty_price_per_100,"accrued_per_100":p.accrued_per_100,"ytm_decimal":p.ytm_decimal,"z_spread_bps":p.z_spread_bps,"oas_bps":p.oas_bps},
        "yield_risk":{"macaulay_duration":q.macaulay_duration,"modified_duration":q.modified_duration,"convexity":q.convexity,"dv01_loss":q.dv01_loss}}),
    )
}
