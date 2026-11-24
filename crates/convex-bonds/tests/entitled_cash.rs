//! Cash entitled between two settlement dates. Every expected amount is worked by hand
//! from the bond's terms, not by the functions under test.

use convex_bonds::cashflows::entitled_cash;
use convex_bonds::contracts::{
    BondContract, BondContractError, BondCoupon, BondProfiles, TermRateTerms,
};
use convex_bonds::instruments::{
    CallableBond, FixedRateBond, FixedRateBondBuilder, FloatingRateNote,
};
use convex_bonds::traits::Bond;
use convex_bonds::types::{CalendarId, CallEntry, CallSchedule, CallType, RateIndex};
use convex_bonds::BondError;
use convex_core::calendars::BusinessDayConvention;
use convex_core::cashflows::{DateOffset, ExCouponBoundary, ExCouponRule, ExDateAnchor};
use convex_core::contracts::IndexId;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;
use convex_core::{Currency, Date};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn d(text: &str) -> Date {
    Date::parse(text).unwrap()
}

/// 5% semiannual, 30/360, coupons on 15 June and 15 December, maturing 15 June 2028: each
/// coupon is 2.5 per 100, and none of these dates is a SIFMA holiday or weekend.
fn five_percent_2028() -> FixedRateBondBuilder {
    FixedRateBond::builder()
        .cusip_unchecked("ENTITLED-5-28")
        .coupon_rate(dec!(0.05))
        .issue_date(d("2025-06-16"))
        .dated_date(d("2025-06-15"))
        .maturity(d("2028-06-15"))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCountConvention::Thirty360US)
}

fn bond() -> FixedRateBond {
    five_percent_2028().build().unwrap()
}

/// The same bond going ex-coupon seven calendar days before each coupon date: the
/// 15 December 2026 coupon goes ex on 8 December.
fn ex_coupon_bond() -> FixedRateBond {
    five_percent_2028()
        .ex_coupon_rule(ExCouponRule {
            offset: DateOffset::CalendarDays(-7),
            anchor: ExDateAnchor::ContractualEnd,
            boundary: ExCouponBoundary::OnDate,
        })
        .build()
        .unwrap()
}

/// A floating rate note on the bond's own schedule and dates, a term rate reset each
/// period plus 20 basis points: what a server builds for a quoted floater.
fn floater(bond: &FixedRateBond) -> BondContract {
    let mut terms = bond.contract_terms().unwrap();
    let period = terms.profiles.coupon[0];
    terms.profiles = BondProfiles::constant(
        period.start,
        period.end,
        Decimal::ZERO,
        dec!(0.002),
        Decimal::ONE,
        bond.face_value(),
    );
    terms.coupon = BondCoupon::Term(TermRateTerms {
        index_id: IndexId::new("SUPPLIED.TERM").unwrap(),
        resets: terms.schedule.clone(),
        reset_adjustment: BusinessDayConvention::Unadjusted,
        fixing_offset: DateOffset::BusinessDays(-2),
        projection_day_count: DayCountConvention::Act360,
    });
    terms.build().unwrap()
}

fn cash(bond: &impl Bond, from: &str, to: &str) -> Decimal {
    entitled_cash(bond, d(from), d(to)).unwrap()
}

fn is_unsupported<T>(result: Result<T, BondContractError>) -> bool {
    matches!(result, Err(BondContractError::Unsupported { .. }))
}

#[test]
fn no_cash_between_the_same_dates_or_between_coupons() {
    assert_eq!(cash(&bond(), "2026-12-14", "2026-12-14"), Decimal::ZERO);
    assert_eq!(cash(&bond(), "2026-12-01", "2026-12-14"), Decimal::ZERO);
}

#[test]
fn a_coupon_paid_between_the_dates_is_owed() {
    assert_eq!(cash(&bond(), "2026-12-14", "2026-12-16"), dec!(2.5));
    // Settling on the coupon date, the holder is no longer owed it.
    assert_eq!(cash(&bond(), "2026-12-14", "2026-12-15"), dec!(2.5));
    assert_eq!(cash(&bond(), "2026-12-15", "2026-12-16"), Decimal::ZERO);
    assert_eq!(cash(&bond(), "2026-12-14", "2027-06-16"), dec!(5));
}

#[test]
fn a_coupon_gone_ex_between_the_dates_is_owed_though_paid_later() {
    let bond = ex_coupon_bond();
    assert_eq!(cash(&bond, "2026-12-07", "2026-12-08"), dec!(2.5));
    assert_eq!(cash(&bond, "2026-12-07", "2026-12-16"), dec!(2.5));
    // Already ex at the earlier date: its holder was never owed it.
    assert_eq!(cash(&bond, "2026-12-08", "2026-12-16"), Decimal::ZERO);
}

#[test]
fn the_redemption_and_last_coupon_are_owed_at_maturity() {
    assert_eq!(cash(&bond(), "2028-06-14", "2028-06-16"), dec!(102.5));
}

#[test]
fn cash_is_per_100_of_the_bonds_face() {
    let bond = five_percent_2028()
        .face_value(dec!(1000))
        .redemption_value(dec!(1000))
        .build()
        .unwrap();
    assert_eq!(cash(&bond, "2026-12-14", "2026-12-16"), dec!(2.5));
    assert_eq!(cash(&bond, "2028-06-14", "2028-06-16"), dec!(102.5));
}

#[test]
fn a_callable_is_owed_its_bonds_cash() {
    let callable = CallableBond::new(
        bond(),
        CallSchedule::new(CallType::Bermudan).with_entry(CallEntry::new(d("2027-06-15"), 101.0)),
    );
    assert_eq!(cash(&callable, "2026-12-14", "2026-12-16"), dec!(2.5));
    assert_eq!(cash(&callable, "2028-06-14", "2028-06-16"), dec!(102.5));
}

const DATE_PAIRS: [(&str, &str); 9] = [
    ("2026-12-14", "2026-12-14"),
    ("2026-12-01", "2026-12-14"),
    ("2026-12-14", "2026-12-15"),
    ("2026-12-15", "2026-12-16"),
    ("2026-12-14", "2027-06-16"),
    ("2026-12-07", "2026-12-08"),
    ("2026-12-08", "2026-12-16"),
    ("2028-06-14", "2028-06-16"),
    ("2025-06-16", "2028-06-16"),
];

#[test]
fn a_fixed_bonds_contract_is_owed_the_bonds_cash() {
    for bond in [bond(), ex_coupon_bond()] {
        let contract = bond.contract_terms().unwrap().build().unwrap();
        for (from, to) in DATE_PAIRS {
            assert_eq!(
                contract.entitled_cash(d(from), d(to)).unwrap(),
                cash(&bond, from, to),
                "{from} to {to}"
            );
        }
    }
}

#[test]
fn a_floater_needs_a_fixing_only_for_a_coupon_between_the_dates() {
    let note = floater(&bond());
    assert_eq!(
        note.entitled_cash(d("2026-12-14"), d("2026-12-14"))
            .unwrap(),
        Decimal::ZERO
    );
    assert_eq!(
        note.entitled_cash(d("2026-12-01"), d("2026-12-14"))
            .unwrap(),
        Decimal::ZERO
    );
    assert_eq!(
        note.entitled_cash(d("2026-12-15"), d("2026-12-16"))
            .unwrap(),
        Decimal::ZERO
    );
    // Paid between, or on the later date itself.
    assert!(is_unsupported(
        note.entitled_cash(d("2026-12-14"), d("2026-12-16"))
    ));
    assert!(is_unsupported(
        note.entitled_cash(d("2026-12-14"), d("2026-12-15"))
    ));
}

#[test]
fn a_floater_coupon_gone_ex_between_the_dates_needs_its_fixing() {
    let note = floater(&ex_coupon_bond());
    assert!(is_unsupported(
        note.entitled_cash(d("2026-12-07"), d("2026-12-08"))
    ));
    assert_eq!(
        note.entitled_cash(d("2026-12-08"), d("2026-12-16"))
            .unwrap(),
        Decimal::ZERO
    );
}

#[test]
fn cash_is_refused_backwards_or_when_the_bond_cannot_project_it() {
    assert!(matches!(
        entitled_cash(&bond(), d("2026-12-16"), d("2026-12-14")),
        Err(BondError::InvalidSpec { .. })
    ));
    let contract = bond().contract_terms().unwrap().build().unwrap();
    assert!(matches!(
        contract.entitled_cash(d("2026-12-16"), d("2026-12-14")),
        Err(BondContractError::Cashflow(_))
    ));
    let note = FloatingRateNote::builder()
        .cusip_unchecked("ENTITLED-FRN")
        .issue_date(d("2025-01-02"))
        .maturity(d("2028-01-02"))
        .index(RateIndex::Sonia)
        .currency(Currency::GBP)
        .frequency(Frequency::Quarterly)
        .day_count(DayCountConvention::Act365Fixed)
        .calendar(CalendarId::uk())
        .build()
        .unwrap();
    // The same date needs no projection; any other needs the note's fixings.
    assert_eq!(cash(&note, "2026-12-14", "2026-12-14"), Decimal::ZERO);
    assert!(matches!(
        entitled_cash(&note, d("2026-12-14"), d("2026-12-16")),
        Err(BondError::MissingField { .. })
    ));
}
