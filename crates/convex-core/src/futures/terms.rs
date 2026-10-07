use serde::{Deserialize, Serialize};

use crate::contracts::{
    BusinessDate, ExactDecimal, IndexId, InstrumentId, ReferenceDataId, SourceId, UtcTimestamp,
    Version,
};
use crate::daycounts::DayCountConvention;
use crate::Currency;

/// A supplied resource revision, never an instruction to resolve latest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcePin {
    /// Stable internal reference resource identity.
    pub id: ReferenceDataId,
    /// Actual immutable revision.
    pub version: Version,
}

/// Exact security or exchange instrument terms revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentPin {
    /// Provider-neutral instrument identity.
    pub id: InstrumentId,
    /// Actual immutable revision.
    pub version: Version,
}

/// Source meaning is retained independently of freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceQuality {
    /// Facts published by the exchange.
    Exchange,
    /// Actual reference/quote/eligibility facts supplied by a provider.
    Provider,
    /// Fabricated demonstration facts; never executable.
    Synthetic,
    /// Informational or model-derived facts; never executable.
    Indicative,
    /// Archived facts explicitly admitted only for indicative use.
    Historical,
}

/// Exact lineage and supplied data-validity interval; no wall-clock freshness guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceStamp {
    /// Internal resource identity.
    pub id: ReferenceDataId,
    /// Actual source/economic version, checked against master revision on import.
    pub version: Version,
    /// Producing source, independent of vendor symbols.
    pub source: SourceId,
    /// Time of the original source assertion.
    pub source_at: UtcTimestamp,
    /// Earliest time the assertion was available to this caller.
    pub available_at: UtcTimestamp,
    /// Exclusive economic end of data validity, supplied by the owning policy.
    pub valid_until: UtcTimestamp,
    /// Actual versus demonstration classification.
    pub quality: ReferenceQuality,
    /// Bounded evidence reference or permitted content hash.
    pub evidence: String,
}

/// Named exchange month; it need not be the final settlement month (e.g. SR3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractMonth {
    /// Four-digit year in the supported reference range 1900..2200.
    pub year: i32,
    /// Calendar month 1..12, not an inferred code letter.
    pub month: u32,
}

/// Dated outright minimum quotation increment. Spread/block regimes are separate contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickRegime {
    /// Inclusive business date.
    pub from: BusinessDate,
    /// Exclusive business date; None covers the remaining contract life.
    pub until: Option<BusinessDate>,
    /// Exact increment in quoted price/index points, never bp or cash.
    pub increment: ExactDecimal,
}

/// Explicit exchange posting convention; calendar rules are supplied separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DailySettlement {
    /// Immutable settlement calendar.
    pub calendar: ResourcePin,
    /// Exchange-local zone identity, retained without inventing a DST schedule.
    pub time_zone: String,
    /// Currency decimal places used for posting, 0..8.
    pub cash_decimal_places: u32,
}

/// Physical-delivery critical dates, inclusive delivery bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondDelivery {
    /// Exact delivery calendar; not inferred from currency.
    pub calendar: ResourcePin,
    /// First date a short may give intention/notice under this supplied contract.
    pub first_notice: BusinessDate,
    /// First permitted delivery date.
    pub first_delivery: BusinessDate,
    /// Last permitted delivery date.
    pub last_delivery: BusinessDate,
    /// Exact basket revision for this contract revision.
    pub basket: ResourcePin,
}

/// Contracted overnight rate averaging rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateAveraging {
    /// Calendar-day weighted arithmetic mean.
    Arithmetic,
    /// Calendar-day weighted compounding in arrears.
    Compounded,
}

/// Actual rate-futures observation/value interval. Bond CTD terms cannot decode here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RateAccrual {
    /// Realized/remaining daily overnight window, half-open.
    Overnight {
        /// Actual index identity.
        index: IndexId,
        /// Inclusive observation/value start.
        start: BusinessDate,
        /// Exclusive end; never the named month interpreted as a tenor.
        end: BusinessDate,
        /// Exact fixing calendar revision.
        calendar: ResourcePin,
        /// Calendar-day averaging or compounding.
        averaging: RateAveraging,
        /// Explicit ACT/360 or ACT/365F basis.
        day_count: DayCountConvention,
    },
    /// A single term-rate fixing with its own value interval.
    Term {
        /// Actual index identity.
        index: IndexId,
        /// Actual index fixing date.
        fixing: BusinessDate,
        /// Inclusive underlying value start.
        start: BusinessDate,
        /// Exclusive underlying value end.
        end: BusinessDate,
        /// Exact fixing/value calendar revision.
        calendar: ResourcePin,
        /// Explicit ACT/360 or ACT/365F basis.
        day_count: DayCountConvention,
    },
}

/// Futures-to-forward calibration adjustment is always an explicit decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FuturesConvexityPolicy {
    /// No adjustment has been admitted; calibration must refuse this input.
    NotAdmitted,
    /// Explicit zero assumption, with its evidence; not an omitted default.
    ZeroAssumption {
        /// Reason/model evidence for this exact assumption.
        evidence: String,
    },
    /// Caller-supplied decimal-rate additive adjustment, with exact model lineage.
    Supplied {
        /// Add this decimal rate to the rate implied by 100 minus price.
        decimal_rate: ExactDecimal,
        /// Model/calibration assumption revision.
        model: ResourcePin,
        /// Evidence for the supplied adjustment.
        evidence: String,
    },
}

/// Initial disjoint payoff/reference families. Each payload rejects extra fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FuturesSettlement {
    /// Government-bond future quoted as clean points per100 face, physically delivered.
    Bond {
        /// Deliverable face amount per contract in contract currency.
        contract_face: ExactDecimal,
        /// Exact delivery and basket terms.
        delivery: BondDelivery,
    },
    /// Cash-settled rate future quoted in index points as100 minus annual percent rate.
    Rate {
        /// Actual final cash settlement date.
        final_settlement: BusinessDate,
        /// Actual contracted observation/value window.
        accrual: RateAccrual,
        /// Explicit calibration adjustment decision.
        convexity: FuturesConvexityPolicy,
    },
}

/// Complete dated exchange reference facts for one immutable contract revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FuturesContract {
    /// Stable internal instrument identity; independent of symbol remaps.
    pub instrument_id: InstrumentId,
    /// Exact reference lineage and revision.
    pub stamp: ReferenceStamp,
    /// Actual exchange identity.
    pub exchange: SourceId,
    /// Exchange product code, e.g. ZN or SR3.
    pub product_code: String,
    /// Exact dated exchange contract code, not a root symbol.
    pub contract_code: String,
    /// Named exchange month, independently retained.
    pub contract_month: ContractMonth,
    /// Contract settlement and quotation currency.
    pub currency: Currency,
    /// Earliest admitted trading date for this reference revision; may follow exchange listing.
    pub listed_from: BusinessDate,
    /// Last trading date. Eligibility conservatively closes at the start of this day.
    pub last_trade: BusinessDate,
    /// Actual expiry date; does not replace delivery or observation dates.
    pub expiry: BusinessDate,
    /// Signed price changes times this value give currency per contract.
    pub currency_per_price_point: ExactDecimal,
    /// Complete contiguous dated outright tick regimes.
    pub ticks: Vec<TickRegime>,
    /// Explicit daily settlement posting conventions.
    pub daily_settlement: DailySettlement,
    /// Disjoint actual payoff/reference terms.
    pub settlement: FuturesSettlement,
}

/// One actual eligible bond and its published conversion factor, never computed from a proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasketBond {
    /// Exact actual bond terms revision.
    pub instrument: InstrumentPin,
    /// Published factor for this bond and this contract month.
    pub conversion_factor: ExactDecimal,
}

/// Immutable delivery universe for one exact physical futures revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryBasket {
    /// Exact source/validity/classification and basket revision.
    pub stamp: ReferenceStamp,
    /// Exact owner contract revision.
    pub contract: InstrumentPin,
    /// Named month retained to reject accidental front/back contract mixing.
    pub contract_month: ContractMonth,
    /// Currency of every deliverable bond.
    pub currency: Currency,
    /// True only when the source explicitly supplies the entire eligible basket.
    pub complete: bool,
    /// Actual eligible IDs/versions and their exact published factors;1..128.
    pub bonds: Vec<BasketBond>,
}

/// Actual per-security funding terms, independent of a valuation curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinancingInput {
    /// Funding source version, validity and quality.
    pub stamp: ReferenceStamp,
    /// Exact financed security revision.
    pub instrument: InstrumentPin,
    /// Actual funding currency, explicitly matched to the cash/security currency.
    pub currency: Currency,
    /// Inclusive start of funding.
    pub start: BusinessDate,
    /// Inclusive funding availability end.
    pub end: BusinessDate,
    /// Annualized simple repo/funding rate, decimal; negative rates are allowed.
    pub repo_decimal: ExactDecimal,
    /// Explicit simple-money-market basis.
    pub day_count: DayCountConvention,
    /// Fraction of dirty purchase value funded with own cash, in[0,1).
    pub haircut_fraction: ExactDecimal,
}

/// Quoted trading costs in currency per100 face, with separate provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashTradingCost {
    /// Actual or estimated cost input; indicative estimates remain non-executable.
    pub stamp: ReferenceStamp,
    /// Nonnegative currency per100 face, separate from bid/ask spread.
    pub currency_per100_face: ExactDecimal,
}

/// Supplied borrowing permission and available face; absence never implies shortability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BorrowInput {
    /// Source/version/freshness and actual versus indicative status.
    pub stamp: ReferenceStamp,
    /// Face available to borrow, nonnegative.
    pub available_face: ExactDecimal,
    /// Last supported borrowing date, inclusive.
    pub until: BusinessDate,
    /// Explicit annualized decimal borrowing fee, nonnegative.
    pub fee_decimal: ExactDecimal,
    /// Explicit simple-money-market basis for the annualized borrowing fee.
    pub day_count: DayCountConvention,
}

/// Cash hedge inputs for one actual security; no synthetic country/tenor bond.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashHedgeCandidate {
    /// Actual immutable security terms revision.
    pub instrument: InstrumentPin,
    /// Actual quote source/version/validity and quality.
    pub quote: ReferenceStamp,
    /// Quotation and funding currency.
    pub currency: Currency,
    /// Actual quoted settlement date.
    pub settlement: BusinessDate,
    /// Minimum exact face trade increment.
    pub lot_face: ExactDecimal,
    /// Executable clean bid per100; None means missing, never zero.
    pub bid_per100: Option<ExactDecimal>,
    /// Executable clean ask per100.
    pub ask_per100: Option<ExactDecimal>,
    /// Face executable at bid, nonnegative; required when selling.
    pub bid_size_face: Option<ExactDecimal>,
    /// Face executable at ask, nonnegative; required when buying.
    pub ask_size_face: Option<ExactDecimal>,
    /// Explicit security-specific financing. Missing funding blocks admission.
    pub financing: Option<FinancingInput>,
    /// Explicit borrow availability. Mandatory for a proposed short.
    pub borrow: Option<BorrowInput>,
    /// Separate actual costs. Missing/estimated costs block execution input admission.
    pub costs: Option<CashTradingCost>,
}

/// Bounded actual cash hedge universe; retains all rejected candidates for diagnosis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashHedgeUniverse {
    /// Exact universe revision, source quality and validity.
    pub stamp: ReferenceStamp,
    /// Explicit universe currency; cross-currency hedging needs a separate policy.
    pub currency: Currency,
    /// Actual candidate facts,1..128 with unique internal security IDs.
    pub candidates: Vec<CashHedgeCandidate>,
}
