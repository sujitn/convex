//! Thin checked constant-coupon accrual with actual calendar version binding.
use super::*;
use convex_bonds::cashflows::AccruedInterestCalculator as A;
use convex_core::{contracts::ExactDecimal, daycounts::DayCountConvention, types::Frequency};
use convex_rates::ReferencePin;
use serde_json::json;

/// Period interest only; full paid/entitlement/step state uses original BondTerms.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccruedInterestOperation {
    /// Interest to settlement inside the original constant-coupon period.
    Standard {
        /// Actual requested date, including either period endpoint.
        settlement: Date,
        /// Original accrual start.
        period_start: Date,
        /// Original accrual end.
        period_end: Date,
        /// Original annual decimal coupon rate, signed or zero.
        annual_rate: ExactDecimal,
        /// Actual positive face; results retain these money units.
        original_face: ExactDecimal,
        /// Explicit supported accrual law.
        day_count: DayCountConvention,
        /// Original coupon frequency, or explicit no-coupon Zero.
        frequency: Frequency,
    },
    /// Remaining-interest ex-coupon law at/on the actual supplied calendar cutoff.
    ExDividend {
        /// Actual requested date within the original accrual period.
        settlement: Date,
        /// Original accrual start.
        period_start: Date,
        /// Original accrual end and explicit ex-date counting anchor.
        period_end: Date,
        /// Actual annual decimal coupon rate.
        annual_rate: ExactDecimal,
        /// Actual positive original face.
        original_face: ExactDecimal,
        /// Explicit supported accrual law.
        day_count: DayCountConvention,
        /// Actual coupon frequency.
        frequency: Frequency,
        /// Business days backwards, checked against the native bound.
        ex_dividend_days: u32,
        /// Exact supplied calendar identity/revision in LibraryInputs.
        calendar: ReferencePin,
    },
    /// Stub interest with complete single-reference coverage of the original period.
    IrregularPeriod {
        /// Actual requested date within original period bounds.
        settlement: Date,
        /// Original actual accrual start.
        period_start: Date,
        /// Original actual accrual end; it cannot be ignored.
        period_end: Date,
        /// Actual original nominal reference start.
        reference_start: Date,
        /// Actual original nominal reference end.
        reference_end: Date,
        /// Actual annual decimal coupon rate.
        annual_rate: ExactDecimal,
        /// Actual positive original face.
        original_face: ExactDecimal,
        /// Supported law; ICMA requires a regular single supplied reference.
        day_count: DayCountConvention,
        /// Actual original coupon frequency.
        frequency: Frequency,
    },
    /// Explicit context-free annual day-fraction interest, with no guessed frequency.
    YearFraction {
        /// Actual requested end date.
        settlement: Date,
        /// Actual start date.
        period_start: Date,
        /// Actual annual decimal coupon rate.
        annual_rate: ExactDecimal,
        /// Actual positive original face.
        original_face: ExactDecimal,
        /// Explicit context-free law; contextual conventions fail.
        day_count: DayCountConvention,
    },
}
pub(super) fn execute(
    action: &AccruedInterestOperation,
    resources: &inputs::ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    use AccruedInterestOperation::*;
    let result = match action {
        Standard {
            settlement,
            period_start,
            period_end,
            annual_rate,
            original_face,
            day_count,
            frequency,
        } => A::standard(
            *settlement,
            *period_start,
            *period_end,
            annual_rate.decimal(),
            original_face.decimal(),
            *day_count,
            *frequency,
        ),
        ExDividend {
            settlement,
            period_start,
            period_end,
            annual_rate,
            original_face,
            day_count,
            frequency,
            ex_dividend_days,
            calendar,
        } => A::ex_dividend(
            *settlement,
            *period_start,
            *period_end,
            annual_rate.decimal(),
            original_face.decimal(),
            *day_count,
            *frequency,
            *ex_dividend_days,
            resources.calendar(calendar)?,
        ),
        IrregularPeriod {
            settlement,
            period_start,
            period_end,
            reference_start,
            reference_end,
            annual_rate,
            original_face,
            day_count,
            frequency,
        } => A::irregular_period(
            *settlement,
            *period_start,
            *period_end,
            *reference_start,
            *reference_end,
            annual_rate.decimal(),
            original_face.decimal(),
            *day_count,
            *frequency,
        ),
        YearFraction {
            settlement,
            period_start,
            annual_rate,
            original_face,
            day_count,
        } => A::using_year_fraction(
            *settlement,
            *period_start,
            annual_rate.decimal(),
            original_face.decimal(),
            *day_count,
        ),
    };
    result
        .map(|amount| json!({"unit":"input_face_cash","amount":ExactDecimal::new(amount)}))
        .map_err(|error| LibraryError {
            code: LibraryErrorCode::InvalidInput,
            message: error.to_string(),
            details: json!({"native_kind":format!("{error:?}")}),
        })
}
